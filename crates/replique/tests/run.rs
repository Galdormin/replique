//! Snapshots of what running each valid file of `tests/corpus` gives.
//!
//! Where `corpus.rs` shows the tree a source parses into, this shows the walk
//! through it: the events the host is handed, in order, and the variables the
//! dialogue writes along the way.
//!
//! The run is deterministic, so the snapshot is stable: every line is
//! acknowledged with [`ResumeEvent::Advance`], and every choice is answered
//! with its first option. Only one path of a branching file is covered, which
//! is the point — the snapshot reads like a transcript.

use replique::RepliqueFile;
use replique::dialogue::Value;
use replique::vm::{DialogueEvent, DialogueVm, ResumeEvent};
use std::collections::BTreeMap;
use std::fmt::Write;

/// Events walked before the run is called a loop. Well above what a corpus
/// file needs, and low enough that a mistake fails instead of hanging.
const MAX_EVENTS: usize = 200;

fn trace(src: &str, entry: &str) -> String {
    let file = RepliqueFile::from_source(src);
    let dialogue = file
        .dialogue
        .expect("a valid corpus file always compiles to a dialogue");

    let mut out = String::new();
    let mut vm = DialogueVm::default();
    let mut known: BTreeMap<String, Value> = BTreeMap::new();
    let mut event = vm.start(dialogue, entry);

    for _ in 0..MAX_EVENTS {
        render_writes(&mut out, &mut known, &vm);

        event = match event {
            Ok(DialogueEvent::Say { speaker, text }) => {
                let speaker = speaker.unwrap_or_else(|| "-".to_owned());
                let _ = writeln!(out, "say {speaker} {text:?}");
                vm.resume(ResumeEvent::Advance)
            }
            Ok(DialogueEvent::Command { name, args }) => {
                let args: Vec<_> = args.iter().map(render_value).collect();
                let _ = writeln!(out, "command {name}({})", args.join(", "));
                vm.resume(ResumeEvent::Advance)
            }
            Ok(DialogueEvent::Choices { choices }) => {
                let _ = writeln!(out, "choices {choices:?}");
                let _ = writeln!(out, "  pick 0 {:?}", choices[0]);
                vm.resume(ResumeEvent::Select(0))
            }
            Ok(DialogueEvent::Finished) => {
                out.push_str("end\n");
                return out;
            }
            Err(err) => {
                let _ = writeln!(out, "error {err}");
                return out;
            }
        };
    }

    out.push_str("(too many events)\n");
    out
}

/// What the dialogue wrote since the last event, in a stable order
fn render_writes(out: &mut String, known: &mut BTreeMap<String, Value>, vm: &DialogueVm) {
    let now: BTreeMap<_, _> = vm
        .vars()
        .iter()
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();

    for (name, value) in &now {
        if known.get(name) != Some(value) {
            let _ = writeln!(out, "  ${name} = {}", render_value(value));
        }
    }

    *known = now;
}

fn render_value(value: &Value) -> String {
    match value {
        Value::Bool(val) => val.to_string(),
        Value::Int(val) => val.to_string(),
        Value::Float(val) => val.to_string(),
        Value::String(val) => format!("{val:?}"),
        Value::Dict(map) => {
            // Sorted, because a `HashMap` gives its entries in no set order
            // and the snapshot must not change from a run to the next.
            let mut attrs = map
                .iter()
                .map(|(key, val)| format!("{key}: {}", render_value(val)))
                .collect::<Vec<_>>();
            attrs.sort();
            format!("{{{}}}", attrs.join(", "))
        }
    }
}

macro_rules! run {
    ($(($name:ident, $entry:literal)),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                let src = include_str!(concat!("corpus/", stringify!($name), ".rep"));
                insta::assert_snapshot!(trace(src, $entry));
            }
        )*
    };
}

run!(
    (minimal, "start"),
    (example, "start"),
    (nested_choices, "start"),
    (empty_body, "start"),
    (two_groups, "start"),
    (unicode, "depart"),
    (forward_jump, "start"),
    (jump_to_end, "start"),
    (commands, "start"),
    (lets, "start"),
    (conditions, "start"),
    (else_branch, "start"),
    (dicts, "start"),
);
