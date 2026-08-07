; Highlights query for tree-sitter-buff.
;
; Captures follow the tree-sitter standard naming conventions:
; https://tree-sitter.github.io/tree-sitter/using-parsers/queries/#highlights
;
; Captures used:
;   @keyword         - reserved keywords (func, if, match, etc.)
;   @keyword.modifier- async, extern, mut, unsafe
;   @keyword.operator- operator-like keywords (in, as)
;   @function        - function declarations & calls
;   @function.call   - function call sites
;   @method          - method calls
;   @type            - type names (PascalCase identifiers)
;   @variable        - identifiers
;   @variable.builtin- true, false, self
;   @constant        - enum variants (UPPER_CASE / PascalCase)
;   @number          - numeric literals
;   @string          - string literals
;   @string.special  - escape sequences, interpolation
;   @operator        - operators
;   @punctuation     - delimiters
;   @comment         - comments
;   @attribute       - @name attributes

; -----------------------------------------------------------------------------
; Comments
; -----------------------------------------------------------------------------
(comment) @comment

; -----------------------------------------------------------------------------
; Attributes
; -----------------------------------------------------------------------------
(attribute
  name: (identifier) @attribute)
 "@" @attribute

; -----------------------------------------------------------------------------
; Keywords (string-literal tokens — match by text)
; -----------------------------------------------------------------------------
[
  "func" "let" "enum" "trait" "if" "else" "for" "while" "return"
  "match" "import" "export" "from" "spawn" "guard" "defer" "extend"
  "struct" "type" "impl"
] @keyword

; break_statement / continue_statement are named nodes (single-token rules);
; highlight the whole node.
(break_statement) @keyword
(continue_statement) @keyword

"in" @keyword.operator
"as" @keyword.operator
"crate" @keyword.operator

[
  "async" "extern" "unsafe"
] @keyword.modifier

"mut" @keyword.modifier

; Booleans are values, not keywords, but highlight as constant.builtin
[
  "true" "false"
] @constant.builtin.boolean

; -----------------------------------------------------------------------------
; Function declarations and calls
; -----------------------------------------------------------------------------
(function_declaration
  name: (identifier) @function)

(trait_method_required
  name: (identifier) @function)

(trait_method_default
  name: (identifier) @function)

(call_expression
  function: (primary_expression
    (identifier) @function.call))

(call_expression
  function: (primary_expression
    (identifier) @function.call))

(method_call_expression
  method: (identifier) @method.call)

; -----------------------------------------------------------------------------
; Types (PascalCase)
; -----------------------------------------------------------------------------
(type_identifier) @type
(named_type (type_identifier) @type)
(generic_type base: (named_type (type_identifier) @type))

; Enum variants
(enum_variant
  name: (identifier) @constant)

(variant_pattern
  variant: (identifier) @constant)

; -----------------------------------------------------------------------------
; Variables
; -----------------------------------------------------------------------------
(parameter
  name: (identifier) @variable.parameter)

(let_declaration
  pattern: (identifier_pattern
    (identifier) @variable))

(struct_pattern_field
  name: (identifier) @variable)

; `self` receiver (Buff's extension-method convention)
(parameter
  name: (identifier) @variable.builtin
  (#eq? @variable.builtin "self"))

; -----------------------------------------------------------------------------
; Literals
; -----------------------------------------------------------------------------
(integer_literal) @number
(float_literal) @number.float
(boolean) @constant.builtin.boolean
(char_literal) @character

(string) @string
(string_fragment) @string.special
(escape_sequence) @string.escape
(interpolation
  "{" @punctuation.special
  "}" @punctuation.special)

; -----------------------------------------------------------------------------
; Operators
; -----------------------------------------------------------------------------
[
  "+" "-" "*" "/" "%"
  "==" "!=" "<" ">" "<=" ">="
  "&&" "||" "!"
  "&" "|" "^" "~" "<<" ">>"
  "->" "=>" "=" "+=" "-=" "*=" "/=" "%="
  ".." "..="
  "|>" "?."
  ; BUG-4: word-operator aliases for `&&`/`||`/!`.
  "and" "or" "not"
] @operator

; -----------------------------------------------------------------------------
; Punctuation
; -----------------------------------------------------------------------------
[
  "(" ")" "[" "]" "{" "}"
  "," "." ":" ";"
] @punctuation.delimiter

"@" @attribute

; Wildcard pattern
(wildcard_pattern) @character.special
