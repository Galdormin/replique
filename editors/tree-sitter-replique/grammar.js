/**
 * @file Grammar for the Replique dialogue format
 * @license MIT OR Apache-2.0
 */

/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

// The format is line-oriented: a line is classified by the marker it starts
// with, and indentation only nests blocks. This grammar mirrors that first
// pass and stays flat -- see README.md for why.

// A run of text up to the end of the line, stopping before a `//` comment or
// the `[` of an inline expression.
const REST_OF_LINE = /([^\n\[/]|\/[^\/\n])+/;

// Same, but stopping at the first `:` as well: what precedes a speaker colon.
const UNTIL_COLON = /([^\n:\[/]|\/[^\/\n:])*/;

// First character of a line that carries text rather than a marker. A marker
// character is allowed when the one after it cannot complete a marker: `-> a`
// is a choice, `-5 degrees` is text.
const TEXT_HEAD = /[^\n\-=:>\[/ \t]|[-=>][^-=:>\n]|\/[^\/\n]/;

module.exports = grammar({
  name: "replique",

  word: ($) => $.identifier,

  // Newlines are significant, so only blanks and comments are skipped.
  extras: ($) => [/[ \t]/, $.comment],

  rules: {
    source_file: ($) => repeat(choice($._statement, $._newline)),

    _newline: (_) => token(/\r?\n/),

    comment: (_) => token(/\/\/[^\n]*/),

    _statement: ($) =>
      choice(
        $.node_start,
        $.node_end,
        $.jump,
        $.choice,
        $.command,
        $.let_statement,
        $.if_statement,
        $.elif_statement,
        $.else_statement,
        $.while_statement,
        $.break_statement,
        $.continue_statement,
        $.say,
      ),

    // := start
    node_start: ($) => seq(":=", field("name", $.node_name)),

    // ---
    node_end: (_) => "---",

    // => meeting
    jump: ($) => seq("=>", field("target", $.node_name)),

    node_name: ($) => $.identifier,

    // -> I can help if you want.
    // `prec.right` keeps an inline expression that opens the text attached to
    // the marker, instead of reading it as a line of its own.
    choice: ($) => prec.right(seq("->", optional(field("text", $.text)))),

    // >> play("bell", 0.5)
    command: ($) => seq(">>", field("call", $.call)),

    // [let $gold = $gold + 1] or [let $player.stats.hp = 1]
    let_statement: ($) =>
      seq(
        "[let",
        field("name", $.variable),
        repeat(field("attribute", $.attribute)),
        "=",
        field("value", $._expression),
        "]",
      ),

    // [if $gold > 5]
    if_statement: ($) => seq("[if", field("condition", $._expression), "]"),
    elif_statement: ($) => seq("[elif", field("condition", $._expression), "]"),
    else_statement: (_) => seq("[else", "]"),

    // [while $gold < 5]
    while_statement: ($) =>
      seq("[while", field("condition", $._expression), "]"),

    // [break] and [continue], which only mean something inside a `[while]`.
    // Whether they are in one is the business of `replique-lsp`, which has the
    // real AST -- see README.md.
    break_statement: (_) => seq("[break", "]"),
    continue_statement: (_) => seq("[continue", "]"),

    // Alice: Hi! / a line with no speaker
    say: ($) =>
      choice(
        prec.right(
          seq(field("speaker", $.speaker), optional(field("text", $.text))),
        ),
        field("text", alias($._bare_text_line, $.text)),
      ),

    // The trailing `:` belongs to the token: it is what tells a speaker apart
    // from a plain line of text.
    speaker: (_) => token(seq(TEXT_HEAD, UNTIL_COLON, ":")),

    // A line of text with no speaker: either it holds no `:` at all, or it
    // starts with one, which leaves the speaker empty.
    _bare_text: (_) =>
      token(
        choice(
          seq(TEXT_HEAD, UNTIL_COLON),
          seq(/:[^-=:>\n]/, optional(REST_OF_LINE)),
          /[-=:>]/,
        ),
      ),

    // What a line says: runs of plain text and the inline expressions between
    // them. `Bonjour [$nom] !` is three parts.
    text: ($) => prec.right(repeat1(choice($.text_chunk, $.interpolation))),

    // A line with no speaker.
    _bare_text_line: ($) =>
      prec.right(
        seq(
          choice(alias($._bare_text, $.text_chunk), $.interpolation),
          repeat(choice($.text_chunk, $.interpolation)),
        ),
      ),

    // [$nom] or [$argent * 100], read in the middle of a line.
    interpolation: ($) => seq("[", $._expression, "]"),

    // Wins over `_bare_text` when both match, so that the text after a
    // speaker or a `->` stays part of that line instead of starting a new one.
    text_chunk: (_) => token(prec(1, REST_OF_LINE)),

    // --- Expressions ---------------------------------------------------

    _expression: ($) =>
      choice(
        $.variable,
        $.number,
        $.string,
        $.boolean,
        $.call,
        $.identifier,
        $.dict,
        $.attribute_expression,
        $.unary_expression,
        $.binary_expression,
        $.parenthesized_expression,
      ),

    // The lexer requires the `(` to touch the name.
    call: ($) =>
      seq(
        field("name", $.function_name),
        token.immediate("("),
        optional(seq($._expression, repeat(seq(",", $._expression)))),
        ")",
      ),

    function_name: ($) => $.identifier,

    // {name: "Alice", stats: {hp: 10}}
    // No trailing comma: the parser rejects it.
    dict: ($) =>
      seq(
        "{",
        optional(seq($.dict_entry, repeat(seq(",", $.dict_entry)))),
        "}",
      ),

    dict_entry: ($) =>
      seq(field("key", $.dict_key), ":", field("value", $._expression)),

    dict_key: ($) => $.identifier,

    // $player.stats.hp, read on a variable or on what a call gives back.
    // Binds tighter than any operator, so `-$a.hp` negates the attribute.
    attribute_expression: ($) =>
      prec(
        7,
        seq(
          field("base", choice($.variable, $.call)),
          repeat1(field("attribute", $.attribute)),
        ),
      ),

    // The name has to touch the `.`, the `.` itself does not have to touch
    // the base: `$a .hp` reads the same as `$a.hp`.
    attribute: (_) => token(/\.[A-Za-z_][A-Za-z0-9_]*/),

    parenthesized_expression: ($) => seq("(", $._expression, ")"),

    unary_expression: ($) =>
      prec(6, seq(field("operator", choice("not", "!", "-")), $._expression)),

    binary_expression: ($) => {
      const table = [
        [1, choice("or", "||")],
        [2, choice("and", "&&")],
        [3, choice("==", "!=", "<", "<=", ">", ">=")],
        [4, choice("+", "-")],
        [5, choice("*", "/")],
      ];

      return choice(
        ...table.map(([precedence, operator]) =>
          prec.left(
            Number(precedence),
            seq($._expression, field("operator", operator), $._expression),
          ),
        ),
      );
    },

    variable: (_) => token(/\$[A-Za-z_][A-Za-z0-9_]*/),
    number: (_) => token(/[0-9]+(\.[0-9]*)*/),
    string: (_) => token(/"([^"\\]|\\.)*"/),
    boolean: (_) => choice("true", "false"),
    identifier: (_) => /[A-Za-z_][A-Za-z0-9_]*/,
  },
});
