# Parser tests

## `corpus.rs` — snapshots with `insta`

Every `corpus/*.rep` file is parsed, then the AST **and** the diagnostics are rendered as text and compared against `snapshots/corpus__<name>.snap`.

### Adding a case

1. Create `corpus/my_case.rep`.
2. Add `my_case` to the `corpus!` macro list at the bottom of `corpus.rs` (the test, the file and the snapshot all share that name).
3. Generate the snapshot (see below), then **read it**. That is where the behaviour gets validated, not when the test turns green.

### Regenerating the snapshots

To rewrite a snapshot that already passes (after changing the rendering
format, for instance), add `INSTA_FORCE_UPDATE=1`.

Interactive alternative, with `cargo install cargo-insta`:

```sh
cargo insta test --review -p replique
```

## `properties.rs` — proptest

Test with random inputs to check if the parser don't panic or throw away to much of the file.
