; Folds query for tree-sitter-buff.
;
; Specifies which nodes can be collapsed in editors.

; Layout blocks (the function/loop body)
(layout_block) @fold

; Brace blocks
(brace_block) @fold

; Match bodies
(match_body) @fold

; Enum bodies
(enum_body) @fold

(trait_body) @fold

; Multi-line comments
(comment) @fold
