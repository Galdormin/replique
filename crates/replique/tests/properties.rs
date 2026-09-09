use proptest::prelude::*;
use replique::parser::parse;

proptest! {
    #[test]
    fn never_panics_always_terminates(s in ".{0,2000}") {
        let _ = parse(&s);
    }

    #[test]
    fn damage_is_bounded(garbage in ".{0,200}") {
        let src = format!(":= a\n{garbage}\n---\n\n:= b\nAlice: ok\n---\n");
        let parsed = parse(&src);
        let b = parsed.nodes.iter().find(|n| n.name.value == "b");
        prop_assert!(b.is_some_and(|n| n.body.len() == 1));
    }
}
