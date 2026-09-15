use bevy::{
    asset::{Assets, Handle},
    ecs::{
        component::Component,
        entity::Entity,
        message::{MessageCursor, Messages},
        system::Local,
        world::World,
    },
    log::{error, warn},
};
use replique::{
    dialogue::{Dialogue, NodeName},
    vm::{DialogueEvent, DialogueVm, VmError},
};

use crate::{
    asset::RepliqueDialogue,
    call::{DialogueToken, function::DialogueHost},
    message::{
        DialogueChoice, DialogueChoices, DialogueCommand, DialogueFinished, DialogueLine,
        ResumeDialogue, StartDialogue,
    },
};

#[derive(Component)]
pub struct DialogueRunner {
    dialogue: Handle<RepliqueDialogue>,
    vm: DialogueVm,
    pending_start: Option<NodeName>,
    seq: u64,
}

impl DialogueRunner {
    pub fn new(dialogue: Handle<RepliqueDialogue>) -> Self {
        Self {
            dialogue,
            vm: DialogueVm::default(),
            pending_start: None,
            seq: 0,
        }
    }

    /// The ticket of the suspension this runner is on. `runner` is the entity
    pub(crate) fn current_token(&self, runner: Entity) -> DialogueToken {
        DialogueToken::new(self.seq, runner)
    }

    /// Opens the next suspension, which voids the ticket of the previous one.
    pub(crate) fn generate_token(&mut self, runner: Entity) -> DialogueToken {
        self.seq += 1;
        self.current_token(runner)
    }
}

/// Hands the event the VM stopped on to whoever is listening.
fn emit(world: &mut World, token: DialogueToken, event: DialogueEvent) {
    match event {
        DialogueEvent::Say { speaker, text } => {
            world.write_message(DialogueLine {
                token,
                speaker,
                text,
            });
        }
        DialogueEvent::Command {
            name,
            args,
            awaited,
        } => {
            world.write_message(DialogueCommand {
                token,
                name,
                args,
                awaited,
            });
        }
        DialogueEvent::Choices { choices } => {
            world.write_message(DialogueChoices {
                token,
                choices: choices
                    .into_iter()
                    .enumerate()
                    .map(|(index, text)| DialogueChoice { index, text })
                    .collect(),
            });
        }
        DialogueEvent::Finished => {
            world.write_message(DialogueFinished {
                runner: token.runner(),
            });
        }
    }
}

/// Runs the VM of `runner` with a [`DialogueHost`] bound to it, and writes the
/// event it stopped on.
fn run(
    world: &mut World,
    runner: Entity,
    with: impl FnOnce(&mut DialogueVm, &mut DialogueHost) -> Result<DialogueEvent, VmError>,
) {
    let Some(mut component) = world.get_mut::<DialogueRunner>(runner) else {
        error!("Try to run an unknown DialogueRunner");
        return;
    };

    // Generate the token for the response even if we don't need one.
    let token = component.generate_token(runner);
    let mut vm = std::mem::take(&mut component.vm);

    let result = {
        let mut host = DialogueHost::new(world, token);
        with(&mut vm, &mut host)
    };

    if let Some(mut component) = world.get_mut::<DialogueRunner>(runner) {
        component.vm = vm;
    }

    match result {
        Ok(event) => emit(world, token, event),
        Err(err) => error!("{err}"),
    }
}

/// The dialogue of `runner`, cloned, or `None` while its asset loads.
fn loaded_dialogue(world: &World, runner: Entity) -> Option<Dialogue> {
    let handle = &world.get::<DialogueRunner>(runner)?.dialogue;
    let asset = world
        .get_resource::<Assets<RepliqueDialogue>>()?
        .get(handle.id())?;

    Some(asset.dialogue().clone())
}

pub(super) fn start_dialogue(world: &mut World, mut cursor: Local<MessageCursor<StartDialogue>>) {
    let pending: Vec<StartDialogue> = {
        let Some(messages) = world.get_resource::<Messages<StartDialogue>>() else {
            return;
        };
        cursor.read(messages).cloned().collect()
    };

    for start in pending {
        if world.get::<DialogueRunner>(start.runner).is_none() {
            error!("Try to start an unknown DialogueRunner");
            continue;
        }

        let Some(dialogue) = loaded_dialogue(world, start.runner) else {
            if let Some(mut component) = world.get_mut::<DialogueRunner>(start.runner) {
                component.pending_start = Some(start.node.clone().into());
            }
            continue;
        };

        run(world, start.runner, |vm, host| {
            vm.start_with(host, dialogue, &start.node)
        });
    }
}

pub(crate) fn start_pending_dialogue(world: &mut World) {
    let mut query = world.query::<(Entity, &DialogueRunner)>();
    let waiting: Vec<Entity> = query
        .iter(world)
        .filter(|(_, runner)| runner.pending_start.is_some())
        .map(|(entity, _)| entity)
        .collect();

    for entity in waiting {
        let Some(dialogue) = loaded_dialogue(world, entity) else {
            continue;
        };

        let node = world
            .get_mut::<DialogueRunner>(entity)
            .and_then(|mut runner| runner.pending_start.take());
        let Some(node) = node else {
            continue;
        };

        run(world, entity, |vm, host| {
            vm.start_with(host, dialogue, node)
        });
    }
}

pub(crate) fn resume_dialogue(world: &mut World, mut cursor: Local<MessageCursor<ResumeDialogue>>) {
    let pending: Vec<ResumeDialogue> = {
        let Some(messages) = world.get_resource::<Messages<ResumeDialogue>>() else {
            return;
        };
        cursor.read(messages).cloned().collect()
    };

    for resume in pending {
        let runner = resume.token.runner();

        // A runner is resumed with an outdated token
        let stale = world
            .get::<DialogueRunner>(runner)
            .is_some_and(|component| component.current_token(runner) != resume.token);

        if stale {
            warn!("Ignore an outdated ResumeDialogue for a dialogue");
            continue;
        }

        run(world, runner, |vm, host| {
            vm.resume_with(host, resume.input.to_resume_event())
        });
    }
}

#[cfg(test)]
mod tests {
    use bevy::{MinimalPlugins, app::App, asset::AssetPlugin, prelude::*};
    use replique::{RepliqueFile, parser::diagnostic::Color};

    use super::*;
    use crate::{
        call::{CurrentDialogueCall, DialogueCall, function::DialogueFunctionAppExt},
        message::{ResumeInput, StartDialogue},
        plugin::RepliquePlugin,
    };

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), RepliquePlugin));
        app
    }

    /// A dialogue asset built from source, without going through the loader.
    fn add_dialogue(app: &mut App, src: &str) -> Handle<RepliqueDialogue> {
        let file = RepliqueFile::from_source(src);
        assert!(
            !file.has_errors(),
            "{}",
            file.render_diagnostics(Color::Never)
        );

        app.world_mut()
            .resource_mut::<Assets<RepliqueDialogue>>()
            .add(RepliqueDialogue::new(file.dialogue.expect("a dialogue")))
    }

    fn spawn_runner(app: &mut App, dialogue: Handle<RepliqueDialogue>) -> Entity {
        app.world_mut().spawn(DialogueRunner::new(dialogue)).id()
    }

    fn start(app: &mut App, runner: Entity) {
        app.world_mut().write_message(StartDialogue {
            runner,
            node: "start".into(),
        });
        app.update();
    }

    /// The token the line of this frame arrived with.
    fn line_token(app: &App) -> DialogueToken {
        app.world()
            .resource::<Messages<DialogueLine>>()
            .iter_current_update_messages()
            .map(|line| line.token)
            .next()
            .expect("a line this frame")
    }

    /// Answers whatever is suspended, and lets the frame carry it out.
    fn resume(app: &mut App, token: DialogueToken) {
        app.world_mut().write_message(ResumeDialogue {
            token,
            input: ResumeInput::Advance,
        });
        app.update();
    }

    /// The lines written this frame.
    fn lines(app: &App) -> Vec<String> {
        app.world()
            .resource::<Messages<DialogueLine>>()
            .iter_current_update_messages()
            .map(|line| line.text.clone())
            .collect()
    }

    #[test]
    fn a_resume_for_a_suspension_already_left_is_ignored() {
        let mut app = app();
        let dialogue = add_dialogue(
            &mut app,
            ":= start\nAlice: un\nAlice: deux\nAlice: trois\n---\n",
        );
        let runner = spawn_runner(&mut app, dialogue);

        start(&mut app, runner);
        assert_eq!(lines(&app), ["un"]);
        let spent = line_token(&app);

        resume(&mut app, spent);
        assert!(lines(&app).contains(&"deux".to_string()));

        resume(&mut app, spent);
        assert!(!lines(&app).contains(&"trois".to_string()));

        resume(&mut app, DialogueToken::new(2, runner));
        assert!(lines(&app).contains(&"trois".to_string()));
    }

    #[test]
    fn a_function_reads_the_dialogue_that_is_asking() {
        fn who(In(()): In<()>, call: DialogueCall) -> String {
            format!(
                "{} {}",
                call.runner(),
                call.token() == call.token_of(call.runner()).unwrap()
            )
        }

        let mut app = app();
        app.add_dialogue_function_named("who", who);
        let dialogue = add_dialogue(&mut app, ":= start\nAlice: [who()]\n---\n");
        let runner = spawn_runner(&mut app, dialogue);

        start(&mut app, runner);

        assert_eq!(lines(&app), [format!("{runner} true")]);
        // And it was taken back once the answer was given.
        assert!(!app.world().contains_resource::<CurrentDialogueCall>());
    }

    /// `[upper_new("bob")]`, answered by a system that reads nothing.
    fn upper_new(In(text): In<(String,)>) -> String {
        text.0.to_uppercase()
    }

    #[test]
    fn an_inline_expression_is_answered_by_a_registered_function() {
        let mut app = app();
        app.add_dialogue_function_named("upper_new", upper_new);
        let dialogue = add_dialogue(
            &mut app,
            ":= start\nAlice: Bonjour [upper_new(\"bob\")] !\n---\n",
        );
        let runner = spawn_runner(&mut app, dialogue);

        start(&mut app, runner);

        assert_eq!(lines(&app), ["Bonjour BOB !"]);
    }

    /// A function may read the world, which is the whole point of registering
    /// a system rather than a closure.
    #[test]
    fn a_function_reads_the_world_it_is_run_against() {
        #[derive(Resource)]
        struct Gold(i64);

        fn gold(In(()): In<()>, gold: Res<Gold>) -> i64 {
            gold.0
        }

        let mut app = app();
        app.insert_resource(Gold(12))
            .add_dialogue_function_named("gold", gold);
        let dialogue = add_dialogue(&mut app, ":= start\nAlice: [gold()] pièces\n---\n");
        let runner = spawn_runner(&mut app, dialogue);

        start(&mut app, runner);

        assert_eq!(lines(&app), ["12 pièces"]);
    }

    /// The host answers for the whole run, so a function reached from a `[let]`
    /// or a choice is called just the same.
    #[test]
    fn a_function_is_reachable_from_a_let_and_from_a_choice() {
        let mut app = app();
        app.add_dialogue_function_named("upper_new", upper_new);
        let dialogue = add_dialogue(
            &mut app,
            ":= start\n[let $nom = upper_new(\"alice\")]\n-> Parler à [$nom]\n    Bob: Salut.\n---\n",
        );
        let runner = spawn_runner(&mut app, dialogue);

        start(&mut app, runner);

        let choices: Vec<String> = app
            .world()
            .resource::<Messages<DialogueChoices>>()
            .iter_current_update_messages()
            .flat_map(|group| group.choices.iter().map(|c| c.text.clone()))
            .collect();
        assert_eq!(choices, ["Parler à ALICE"]);
    }

    /// A function nobody registered is an error, and the runner stays where it
    /// was rather than skipping the line.
    #[test]
    fn an_unregistered_function_leaves_the_runner_where_it_was() {
        let mut app = app();
        let dialogue = add_dialogue(&mut app, ":= start\nAlice: [upper_new(\"bob\")]\n---\n");
        let runner = spawn_runner(&mut app, dialogue);

        start(&mut app, runner);

        assert_eq!(lines(&app), [] as [String; 0]);

        // The VM is back in its component, untouched, so registering the
        // function and asking again works.
        app.add_dialogue_function_named("upper_new", upper_new);
        start(&mut app, runner);

        assert_eq!(lines(&app), ["BOB"]);
    }

    /// A start asked for before the asset is there is kept, and taken up by
    /// `start_pending_dialogue` on a later frame — with a host, like any other.
    #[test]
    fn a_pending_start_is_taken_up_once_the_asset_is_there() {
        let mut app = app();
        app.add_dialogue_function_named("upper_new", upper_new);
        let handle = add_dialogue(&mut app, ":= start\nAlice: [upper_new(\"bob\")]\n---\n");
        let runner = spawn_runner(&mut app, handle.clone());

        // Taken back out, so the runner holds a handle on an asset that is not
        // there yet, as it would while the file loads.
        let dialogue = app
            .world_mut()
            .resource_mut::<Assets<RepliqueDialogue>>()
            .remove(handle.id())
            .expect("the asset was just added");

        start(&mut app, runner);
        assert_eq!(lines(&app), [] as [String; 0]);
        assert!(
            app.world()
                .get::<DialogueRunner>(runner)
                .unwrap()
                .pending_start
                .is_some()
        );

        app.world_mut()
            .resource_mut::<Assets<RepliqueDialogue>>()
            .insert(handle.id(), dialogue)
            .expect("the handle is still alive");
        app.update();

        assert_eq!(lines(&app), ["BOB"]);
        assert!(
            app.world()
                .get::<DialogueRunner>(runner)
                .unwrap()
                .pending_start
                .is_none()
        );
    }

    /// A function that answers a `Result` fails the call with its own message
    /// instead of putting something made up in the line.
    #[test]
    fn a_function_can_refuse_to_answer() {
        fn stat(In((who,)): In<(String,)>) -> Result<i64, String> {
            match who.as_str() {
                "Alice" => Ok(14),
                other => Err(format!("{other} is not on the scene")),
            }
        }

        let mut app = app();
        app.add_dialogue_function_named("stat", stat);
        let dialogue = add_dialogue(
            &mut app,
            ":= start\nAlice: [stat(Alice)] and [stat(Carol)]\n---\n",
        );
        let runner = spawn_runner(&mut app, dialogue);

        start(&mut app, runner);

        assert_eq!(lines(&app), [] as [String; 0]);
    }

    /// The same function, when it does answer, reads as the value it holds.
    #[test]
    fn a_function_that_answers_a_result_reads_as_its_value() {
        fn stat(In((_who,)): In<(String,)>) -> Result<i64, String> {
            Ok(14)
        }

        let mut app = app();
        app.add_dialogue_function_named("stat", stat);
        let dialogue = add_dialogue(&mut app, ":= start\nAlice: [stat(Alice)] hp\n---\n");
        let runner = spawn_runner(&mut app, dialogue);

        start(&mut app, runner);

        assert_eq!(lines(&app), ["14 hp"]);
    }
}
