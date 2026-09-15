; Line markers
[
  ":="
  "=>"
  "->"
  ">>"
  "[let"
  "[if"
  "[elif"
  "[else"
  "[while"
  "[break"
  "[continue"
  "]"
] @keyword

; `---`
(node_end) @keyword

; `:= name` or `=> name`
(node_name) @enum

; `Alice:` -- the trailing colon belongs to the token
(speaker) @property

; `[$nom]` read in the middle of a line
(interpolation
  [
    "["
    "]"
  ] @punctuation.special)

; Expressions
(variable) @variable.special
(function_name) @function.method
(attribute) @property
(dict_key) @property
(string) @string
(number) @number
(boolean) @boolean
(comment) @comment

[
  "not"
  "and"
  "or"
] @keyword.operator

[
  "!"
  "&&"
  "||"
  "=="
  "!="
  "<"
  "<="
  ">"
  ">="
  "+"
  "-"
  "*"
  "/"
  "="
] @operator

[
  "("
  ")"
  "{"
  "}"
] @punctuation.bracket

[
  ","
  ":"
] @punctuation.delimiter
