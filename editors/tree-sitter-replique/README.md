# tree-sitter-replique

Tree-sitter grammar for the [Replique](../../README.md) dialogue format, used by
the Zed extension in [`../zed`](../zed) for syntax highlighting and the outline.

## Design

The grammar is **flat**: one node per line, no nesting. Replique blocks are
delimited by indentation, which tree-sitter cannot track without an external
scanner written in C. Highlighting and the outline do not need that structure,
so the grammar mirrors the first pass of the Rust parser (`parser::lines`) and
stops there. Nesting stays the business of `replique-lsp`, which has the real
AST.

## Working on it

```sh
npm install --global tree-sitter-cli
tree-sitter generate   # regenerates src/, which is committed on purpose
tree-sitter test       # runs test/corpus
```

`src/parser.c` is committed because Zed compiles it to wasm itself; it must stay
in sync with `grammar.js`.
