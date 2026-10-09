//! Answering the `has` of a dialogue.
//!
//! Asked of a name rather than of a dictionary, `has` is a question about the
//! game: `[if Alice has Angry]` asks whether the entity the dialogue calls
//! `Alice` holds the component it calls `Angry`. Both names have to be given
//! to the plugin, since neither means anything to Bevy.
//!
//! **A subject is an entity with a name.** [`RepliqueSubject`] is the
//! component that gives it one:
//!
//! ```
//! # use bevy::prelude::*;
//! # use bevy_replique::prelude::*;
//! # let mut world = World::new();
//! world.spawn(RepliqueSubject::new("Alice"));
//! ```
//!
//! **A feature is a component with a name.**
//! [`register_replique_feature`](DialogueFeatureAppExt::register_replique_feature)
//! gives it one:
//!
//! ```
//! # use bevy::prelude::*;
//! # use bevy_replique::prelude::*;
//! #[derive(Component)]
//! struct Angry;
//!
//! # let mut app = App::new();
//! app.register_replique_feature::<Angry>("Angry");
//! ```
//!
//! From there `Alice has Angry` is `true` exactly while that entity holds an
//! `Angry`. Only the presence of the component is looked at.
//!
//! A name the plugin was never given is a mistake in the dialogue, not a
//! `false`. An unknown subject or an unknown feature stops the dialogue with
//! an error, so that a typo does not quietly send it down the `[else]`.
//!
//! # The same name in two dialogues
//!
//! A subject is visible to every dialogue unless it says otherwise. One that
//! is [`only_for`](RepliqueSubject::only_for) some runners is seen by those
//! alone, which lets two dialogues running side by side each have their own
//! `Guard`:
//!
//! ```
//! # use bevy::prelude::*;
//! # use bevy_replique::prelude::*;
//! # let mut world = World::new();
//! # let (north_gate, south_gate) = (world.spawn_empty().id(), world.spawn_empty().id());
//! world.spawn(RepliqueSubject::new("Guard").only_for(north_gate));
//! world.spawn(RepliqueSubject::new("Guard").only_for(south_gate));
//! ```
//!
//! A subject scoped to the runner that is asking comes before one visible to
//! all, so a dialogue can also stand in its own `Alice` for the usual one.
//! Two subjects of the same name left at the same level are ambiguous, and
//! asking about them is an error.

use bevy::{
    app::App,
    ecs::{
        component::{Component, ComponentId},
        entity::{Entity, MapEntities},
        lifecycle::Remove,
        observer::On,
        resource::Resource,
        system::Query,
        world::World,
    },
    log::warn,
    platform::collections::HashMap,
};
use replique::host::HostError;

use crate::runner::DialogueRunner;

/// The name a dialogue knows an entity by.
///
/// `Alice` in `[if Alice has Angry]` is the entity holding
/// `RepliqueSubject::new("Alice")`. The name is the one the dialogue writes,
/// case included.
///
/// An entity has one name. See the [module](self) for how the same name can
/// stand for different entities in different dialogues.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct RepliqueSubject {
    name: String,
    #[entities]
    scope: SubjectScope,
}

impl RepliqueSubject {
    /// A subject every dialogue can see.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            scope: SubjectScope::All,
        }
    }

    /// Restricts the subject to the dialogue of `runner`. Called again, it
    /// adds a runner to those that already see it.
    pub fn only_for(mut self, runner: Entity) -> Self {
        self.show_to(runner);
        self
    }

    /// Same as [`only_for`](Self::only_for), on a subject already spawned.
    pub fn show_to(&mut self, runner: Entity) {
        match &mut self.scope {
            SubjectScope::All => self.scope = SubjectScope::Only(vec![runner]),
            SubjectScope::Only(runners) => {
                if !runners.contains(&runner) {
                    runners.push(runner);
                }
            }
        }
    }

    /// Takes the subject away from the dialogue of `runner`.
    ///
    /// Only a scoped subject can be hidden: one visible to all stays so. A
    /// subject hidden from its last runner is visible to none, and does not
    /// go back to being visible to all.
    pub fn hide_from(&mut self, runner: Entity) {
        if let SubjectScope::Only(runners) = &mut self.scope {
            runners.retain(|seen_by| *seen_by != runner);
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn scope(&self) -> &SubjectScope {
        &self.scope
    }
}

/// Visibility of a [`RepliqueSubject`] to the existing [`DialogueRunner`].
#[derive(MapEntities, Clone, Debug, PartialEq, Eq)]
pub enum SubjectScope {
    /// Visible to all runners.
    All,
    /// Visible only to this runners.
    Only(#[entities] Vec<Entity>),
}

/// Indentify a feature in the World
///
/// Only the [`ComponentId`] is kept for now.
#[derive(Clone, Debug)]
pub(crate) struct Feature(ComponentId);

impl Feature {
    fn component_id(&self) -> ComponentId {
        self.0
    }
}

/// The components a dialogue can query.
#[derive(Resource, Default)]
pub(crate) struct DialogueFeatureRegistry {
    features: HashMap<String, Feature>,
}

/// Registers features on an [`App`].
pub trait DialogueFeatureAppExt {
    /// Lets dialogues ask whether a subject holds the component `C`, under
    /// `name`: after `register_replique_feature::<Angry>("Angry")`,
    /// `Alice has Angry` is `true` while the entity named `Alice` holds `Angry`.
    ///
    /// The name is the one the dialogue writes, case included, and does not
    /// have to be the name of the type. A name registered twice keeps the last
    /// component it was given.
    ///
    /// ```
    /// # use bevy::prelude::*;
    /// # use bevy_replique::prelude::*;
    /// #[derive(Component)]
    /// struct Mana(f32);
    ///
    /// # let mut app = App::new();
    /// // `[if Alice has magic]`
    /// app.register_replique_feature::<Mana>("magic");
    /// ```
    fn register_replique_feature<C: Component>(&mut self, name: impl Into<String>) -> &mut Self;
}

impl DialogueFeatureAppExt for App {
    fn register_replique_feature<C: Component>(&mut self, name: impl Into<String>) -> &mut Self {
        let name = name.into();
        let world = self.world_mut();
        let id = world.register_component::<C>();

        let mut registry = world.get_resource_or_init::<DialogueFeatureRegistry>();
        if let Some(previous) = registry.features.insert(name.clone(), Feature(id))
            && previous.component_id() != id
        {
            warn!("The feature {name} was already registered for another component.");
        }

        self
    }
}

/// Whether the entity `runner` knows as `subject` holds the component registered as `feature`.
pub(crate) fn has_feature(
    world: &mut World,
    runner: Entity,
    subject: &str,
    feature: &str,
) -> Result<bool, HostError> {
    let id = world
        .get_resource::<DialogueFeatureRegistry>()
        .and_then(|registry| registry.features.get(feature).cloned())
        .ok_or_else(|| HostError::UnknownFeature(feature.to_owned()))?;

    let entity = resolve_subject(world, runner, subject)?;

    Ok(world.entity(entity).contains_id(id.component_id()))
}

/// The entity `runner` knows as `name`.
///
/// The subjects scoped to `runner` are looked at first, then the ones visible
/// to all. A level holding several of them is an error rather than a pick: the
/// order a query walks entities in is not something a dialogue should depend
/// on.
fn resolve_subject(world: &mut World, runner: Entity, name: &str) -> Result<Entity, HostError> {
    let mut scoped = vec![];
    let mut global = vec![];

    let mut subjects = world.query::<(Entity, &RepliqueSubject)>();
    for (entity, subject) in subjects.iter(world) {
        if subject.name != name {
            continue;
        }

        match &subject.scope {
            SubjectScope::All => global.push(entity),
            SubjectScope::Only(runners) if runners.contains(&runner) => scoped.push(entity),
            SubjectScope::Only(_) => {}
        }
    }

    let candidates = if scoped.is_empty() { global } else { scoped };

    match candidates.as_slice() {
        [] => Err(HostError::UnknownSubject(name.to_owned())),
        [entity] => Ok(*entity),
        several => Err(HostError::Failed {
            name: name.to_owned(),
            message: format!(
                "{} subjects of that name are visible to this dialogue",
                several.len()
            ),
        }),
    }
}

/// Takes a runner that is going away out of the scope of every subject.
///
/// A subject left with no runner stays scoped, and so visible to none: turning
/// it into one visible to all would hand a dialogue an `Alice` that was meant
/// for another.
pub(crate) fn forget_runner(
    removed: On<Remove<DialogueRunner>>,
    mut subjects: Query<&mut RepliqueSubject>,
) {
    let runner = removed.entity;

    for mut subject in &mut subjects {
        let listed = matches!(
            &subject.scope,
            SubjectScope::Only(runners) if runners.contains(&runner)
        );

        // Checked first so that only the subjects that change are marked as changed.
        if listed {
            subject.hide_from(runner);
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::{
        MinimalPlugins,
        asset::{AssetPlugin, Assets, Handle},
        ecs::{entity::EntityHashMap, message::Messages},
    };
    use replique::{RepliqueFile, parser::diagnostic::Color};

    use super::*;
    use crate::{
        asset::RepliqueDialogue,
        message::{DialogueLine, StartDialogue},
        plugin::RepliquePlugin,
    };

    #[derive(Component)]
    struct Angry;

    #[derive(Component)]
    struct Mana(#[allow(unused)] f32);

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), RepliquePlugin));
        app.register_replique_feature::<Angry>("Angry")
            .register_replique_feature::<Mana>("Mana");
        app
    }

    /// An entity standing for a runner: resolving a subject only needs its id.
    fn runner(app: &mut App) -> Entity {
        app.world_mut().spawn_empty().id()
    }

    /// What `has` answered, in a shape that compares: [`HostError`] does not.
    #[derive(Debug, PartialEq)]
    enum Answer {
        Yes,
        No,
        UnknownSubject(String),
        UnknownFeature(String),
        Ambiguous(String),
    }

    fn has(app: &mut App, runner: Entity, subject: &str, feature: &str) -> Answer {
        match has_feature(app.world_mut(), runner, subject, feature) {
            Ok(true) => Answer::Yes,
            Ok(false) => Answer::No,
            Err(HostError::UnknownSubject(name)) => Answer::UnknownSubject(name),
            Err(HostError::UnknownFeature(name)) => Answer::UnknownFeature(name),
            Err(HostError::Failed { name, .. }) => Answer::Ambiguous(name),
            Err(other) => panic!("unexpected error: {other}"),
        }
    }

    fn scope_of(app: &App, subject: Entity) -> SubjectScope {
        app.world()
            .get::<RepliqueSubject>(subject)
            .expect("a subject")
            .scope()
            .clone()
    }

    #[test]
    fn a_subject_has_the_features_it_holds() {
        let mut app = app();
        let runner = runner(&mut app);
        app.world_mut()
            .spawn((RepliqueSubject::new("Alice"), Angry));
        app.world_mut().spawn(RepliqueSubject::new("Bob"));

        assert_eq!(has(&mut app, runner, "Alice", "Angry"), Answer::Yes);
        assert_eq!(has(&mut app, runner, "Alice", "Mana"), Answer::No);
        assert_eq!(has(&mut app, runner, "Bob", "Angry"), Answer::No);
    }

    #[test]
    fn a_feature_is_there_whatever_it_holds() {
        let mut app = app();
        let runner = runner(&mut app);
        app.world_mut()
            .spawn((RepliqueSubject::new("Alice"), Mana(0.0)));

        assert_eq!(has(&mut app, runner, "Alice", "Mana"), Answer::Yes);
    }

    #[test]
    fn a_feature_follows_the_component_as_it_comes_and_goes() {
        let mut app = app();
        let runner = runner(&mut app);
        let alice = app.world_mut().spawn(RepliqueSubject::new("Alice")).id();

        assert_eq!(has(&mut app, runner, "Alice", "Angry"), Answer::No);

        app.world_mut().entity_mut(alice).insert(Angry);
        assert_eq!(has(&mut app, runner, "Alice", "Angry"), Answer::Yes);

        app.world_mut().entity_mut(alice).remove::<Angry>();
        assert_eq!(has(&mut app, runner, "Alice", "Angry"), Answer::No);
    }

    #[test]
    fn a_feature_is_known_by_the_name_it_was_registered_under() {
        let mut app = app();
        app.register_replique_feature::<Angry>("furious");
        let runner = runner(&mut app);
        app.world_mut()
            .spawn((RepliqueSubject::new("Alice"), Angry));

        assert_eq!(has(&mut app, runner, "Alice", "furious"), Answer::Yes);
        assert_eq!(
            has(&mut app, runner, "Alice", "angry"),
            Answer::UnknownFeature("angry".into())
        );
    }

    #[test]
    fn a_name_registered_twice_keeps_the_last_component() {
        let mut app = app();
        app.register_replique_feature::<Mana>("Angry");
        let runner = runner(&mut app);
        app.world_mut()
            .spawn((RepliqueSubject::new("Alice"), Angry));

        assert_eq!(has(&mut app, runner, "Alice", "Angry"), Answer::No);
    }

    #[test]
    fn error_on_a_feature_that_was_never_registered() {
        let mut app = app();
        let runner = runner(&mut app);
        app.world_mut().spawn(RepliqueSubject::new("Alice"));

        assert_eq!(
            has(&mut app, runner, "Alice", "Sleepy"),
            Answer::UnknownFeature("Sleepy".into())
        );
    }

    #[test]
    fn error_on_a_subject_no_entity_is_named_after() {
        let mut app = app();
        let runner = runner(&mut app);
        app.world_mut().spawn(RepliqueSubject::new("Alice"));

        assert_eq!(
            has(&mut app, runner, "alice", "Angry"),
            Answer::UnknownSubject("alice".into())
        );
    }

    #[test]
    fn a_subject_that_was_despawned_is_unknown() {
        let mut app = app();
        let runner = runner(&mut app);
        let alice = app.world_mut().spawn(RepliqueSubject::new("Alice")).id();

        app.world_mut().despawn(alice);

        assert_eq!(
            has(&mut app, runner, "Alice", "Angry"),
            Answer::UnknownSubject("Alice".into())
        );
    }

    #[test]
    fn a_scoped_subject_is_seen_by_its_runners_only() {
        let mut app = app();
        let (north, south, east) = (runner(&mut app), runner(&mut app), runner(&mut app));
        app.world_mut().spawn((
            RepliqueSubject::new("Guard")
                .only_for(north)
                .only_for(south),
            Angry,
        ));

        assert_eq!(has(&mut app, north, "Guard", "Angry"), Answer::Yes);
        assert_eq!(has(&mut app, south, "Guard", "Angry"), Answer::Yes);
        assert_eq!(
            has(&mut app, east, "Guard", "Angry"),
            Answer::UnknownSubject("Guard".into())
        );
    }

    #[test]
    fn two_runners_can_each_have_their_own_subject_of_the_same_name() {
        let mut app = app();
        let (north, south) = (runner(&mut app), runner(&mut app));
        app.world_mut()
            .spawn((RepliqueSubject::new("Guard").only_for(north), Angry));
        app.world_mut()
            .spawn(RepliqueSubject::new("Guard").only_for(south));

        assert_eq!(has(&mut app, north, "Guard", "Angry"), Answer::Yes);
        assert_eq!(has(&mut app, south, "Guard", "Angry"), Answer::No);
    }

    #[test]
    fn a_scoped_subject_comes_before_the_one_visible_to_all() {
        let mut app = app();
        let (north, south) = (runner(&mut app), runner(&mut app));
        app.world_mut().spawn(RepliqueSubject::new("Alice"));
        app.world_mut()
            .spawn((RepliqueSubject::new("Alice").only_for(north), Angry));

        assert_eq!(has(&mut app, north, "Alice", "Angry"), Answer::Yes);
        assert_eq!(has(&mut app, south, "Alice", "Angry"), Answer::No);
    }

    #[test]
    fn error_on_two_subjects_of_the_same_name_at_the_same_level() {
        let mut app = app();
        let north = runner(&mut app);
        app.world_mut().spawn(RepliqueSubject::new("Alice"));
        app.world_mut().spawn(RepliqueSubject::new("Alice"));
        app.world_mut()
            .spawn(RepliqueSubject::new("Guard").only_for(north));
        app.world_mut()
            .spawn(RepliqueSubject::new("Guard").only_for(north));

        for name in ["Alice", "Guard"] {
            assert_eq!(
                has(&mut app, north, name, "Angry"),
                Answer::Ambiguous(name.into())
            );
        }
    }

    /// The feature is checked first, so that the error names the word that is
    /// wrong whichever subject it was asked of.
    #[test]
    fn an_unknown_feature_is_reported_before_an_unknown_subject() {
        let mut app = app();
        let runner = runner(&mut app);

        assert_eq!(
            has(&mut app, runner, "Nobody", "Sleepy"),
            Answer::UnknownFeature("Sleepy".into())
        );
    }

    #[test]
    fn show_to_and_hide_from_edit_the_scope() {
        let mut world = World::new();
        let (north, south) = (world.spawn_empty().id(), world.spawn_empty().id());
        let mut subject = RepliqueSubject::new("Guard");
        assert_eq!(subject.scope(), &SubjectScope::All);

        subject.hide_from(north);
        assert_eq!(subject.scope(), &SubjectScope::All);

        subject.show_to(north);
        subject.show_to(north);
        subject.show_to(south);
        assert_eq!(subject.scope(), &SubjectScope::Only(vec![north, south]));

        subject.hide_from(north);
        subject.hide_from(south);
        assert_eq!(subject.scope(), &SubjectScope::Only(vec![]));
    }

    #[test]
    fn a_runner_that_is_despawned_leaves_the_scope_of_its_subjects() {
        let mut app = app();
        let (north, south) = {
            let world = app.world_mut();
            (
                world.spawn(DialogueRunner::new(Handle::default())).id(),
                world.spawn(DialogueRunner::new(Handle::default())).id(),
            )
        };
        let guard = app
            .world_mut()
            .spawn(
                RepliqueSubject::new("Guard")
                    .only_for(north)
                    .only_for(south),
            )
            .id();
        let alice = app.world_mut().spawn(RepliqueSubject::new("Alice")).id();

        app.world_mut().despawn(north);

        assert_eq!(scope_of(&app, guard), SubjectScope::Only(vec![south]));
        assert_eq!(scope_of(&app, alice), SubjectScope::All);
    }

    #[test]
    fn a_subject_left_with_no_runner_is_visible_to_none() {
        let mut app = app();
        let north = app
            .world_mut()
            .spawn(DialogueRunner::new(Handle::default()))
            .id();
        let south = runner(&mut app);
        let guard = app
            .world_mut()
            .spawn(RepliqueSubject::new("Guard").only_for(north))
            .id();

        app.world_mut().entity_mut(north).remove::<DialogueRunner>();

        assert_eq!(scope_of(&app, guard), SubjectScope::Only(vec![]));
        assert_eq!(
            has(&mut app, south, "Guard", "Angry"),
            Answer::UnknownSubject("Guard".into())
        );
    }

    #[test]
    fn the_runners_of_a_scope_are_remapped_with_the_entities() {
        let mut world = World::new();
        let (old, new) = (world.spawn_empty().id(), world.spawn_empty().id());
        let mut map = EntityHashMap::from_iter([(old, new)]);
        let mut subject = RepliqueSubject::new("Guard").only_for(old);

        <RepliqueSubject as Component>::map_entities(&mut subject, &mut map);

        assert_eq!(subject.scope(), &SubjectScope::Only(vec![new]));
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

    /// Starts `dialogue` on a new runner and gives the lines of that frame.
    fn first_lines(app: &mut App, dialogue: Handle<RepliqueDialogue>) -> Vec<String> {
        let runner = app.world_mut().spawn(DialogueRunner::new(dialogue)).id();
        start(app, runner)
    }

    fn start(app: &mut App, runner: Entity) -> Vec<String> {
        app.world_mut().write_message(StartDialogue {
            runner,
            node: "start".into(),
        });
        app.update();

        app.world()
            .resource::<Messages<DialogueLine>>()
            .iter_current_update_messages()
            .filter(|line| line.token.runner() == runner)
            .map(|line| line.text.clone())
            .collect()
    }

    const MOOD: &str = ":= start
[if Alice has Angry]
    Alice: Furieuse.
[else]
    Alice: Calme.
---
";

    #[test]
    fn a_dialogue_branches_on_the_components_of_a_subject() {
        let mut app = app();
        let dialogue = add_dialogue(&mut app, MOOD);
        let alice = app.world_mut().spawn(RepliqueSubject::new("Alice")).id();

        assert_eq!(first_lines(&mut app, dialogue.clone()), ["Calme."]);

        app.world_mut().entity_mut(alice).insert(Angry);
        assert_eq!(first_lines(&mut app, dialogue), ["Furieuse."]);
    }

    #[test]
    fn a_dialogue_reads_the_subject_scoped_to_its_own_runner() {
        let mut app = app();
        let dialogue = add_dialogue(&mut app, MOOD);
        let (north, south) = {
            let world = app.world_mut();
            (
                world.spawn(DialogueRunner::new(dialogue.clone())).id(),
                world.spawn(DialogueRunner::new(dialogue)).id(),
            )
        };
        app.world_mut()
            .spawn((RepliqueSubject::new("Alice").only_for(north), Angry));
        app.world_mut()
            .spawn(RepliqueSubject::new("Alice").only_for(south));

        assert_eq!(start(&mut app, north), ["Furieuse."]);
        assert_eq!(start(&mut app, south), ["Calme."]);
    }

    /// The error is logged and the dialogue goes nowhere: neither branch is
    /// played on a name the game does not know.
    #[test]
    fn a_dialogue_stops_on_a_subject_it_cannot_find() {
        let mut app = app();
        let dialogue = add_dialogue(&mut app, MOOD);

        assert!(first_lines(&mut app, dialogue).is_empty());
    }
}
