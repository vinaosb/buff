; Locals query for tree-sitter-buff.
;
; Identifies local definitions and references for editors (e.g., for
; highlighting all occurrences of a variable under the cursor).

; -----------------------------------------------------------------------------
; Definitions
; -----------------------------------------------------------------------------

; Function declarations define a function name at the module scope.
(function_declaration
  name: (identifier) @local.definition.function)

; Trait methods.
(trait_method_required
  name: (identifier) @local.definition.method)
(trait_method_default
  name: (identifier) @local.definition.method)

; Parameters define bindings in the function's scope.
(parameter
  name: (identifier) @local.definition.var)

; let-declarations define a variable in the enclosing scope.
(let_declaration
  pattern: (identifier_pattern
    (identifier) @local.definition.var))

; let-declarations with tuple / struct destructuring bind multiple names.
(let_declaration
  pattern: (tuple_pattern
    (identifier_pattern
      (identifier) @local.definition.var)))

(let_declaration
  pattern: (struct_pattern
    (struct_pattern_field
      name: (identifier) @local.definition.var)))

; for-loop variable binding.
(for_statement
  (identifier) @local.definition.var)

; Closure parameters (within `{ params => body }`).
(closure_expression
  (identifier) @local.definition.var)

; -----------------------------------------------------------------------------
; Scopes
; -----------------------------------------------------------------------------

(function_declaration) @local.scope
(layout_block) @local.scope
(brace_block) @local.scope
(match_body) @local.scope

; -----------------------------------------------------------------------------
; References
; -----------------------------------------------------------------------------
; Any identifier that isn't at a definition site is a reference.
(identifier) @local.reference
