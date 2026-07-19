; Indents query for tree-sitter-buff.
;
; Tells editors how to auto-indent Buff code.

[
  (layout_block)
  (brace_block)
] @indent.begin

; Closing delimiters dedent.
"}" @indent.dedent
"]" @indent.dedent
")" @indent.dedent

; Empty lines should be auto-trimmed.
(layout_block
  (dedent) @indent.dedent)
