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
  "]"
] @keyword

; `---`
(node_end) @keyword

; `:= name` or `=> name`
(node_name) @enum

; `Alice:` -- the trailing colon belongs to the token
(speaker) @property

; Expressions
(variable) @variable.special
(function_name) @function.method
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
] @punctuation.bracket

"," @punctuation.delimiter
