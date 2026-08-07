/**
 * @file grammar.js — tree-sitter grammar for the Buff language.
 *
 * Buff is a layout-sensitive language that transpiles to Rust:
 *  - 4-space indentation defines blocks (no braces for control flow).
 *  - Braces `{}` are reserved for data: struct literals, maps, closures,
 *    string interpolation, match arms, enum/trait/extend bodies.
 *  - 30 reserved keywords (see grammar rules below) plus 3 word-operator
 *    aliases `and`/`or`/`not` (BUG-4) that mirror `&&`/`||`/`!`.
 *
 * The external scanner (`src/scanner.c`) implements the offside rule:
 * it emits NEWLINE / INDENT / DEDENT tokens based on leading-whitespace
 * changes between non-blank lines. Tabs are not treated as indent.
 *
 * The hand-rolled parser in `crates/buff-lang-parser/` is the authoritative
 * grammar; this tree-sitter grammar is a derived approximation suitable
 * for editor highlighting and corpus testing.
 */

const PREC = {
  assignment: 1,
  pipeline: 2,
  range: 3,
  null_coalesce: 4,
  logical_or: 5,
  logical_and: 6,
  equality: 7,
  comparison: 8,
  bitwise_or: 9,
  bitwise_xor: 10,
  bitwise_and: 11,
  shift: 12,
  additive: 13,
  multiplicative: 14,
  unary: 15,
  postfix: 16,
  primary: 17,
};

const commaSep = rule => optional(commaSep1(rule));
const commaSep1 = rule => seq(rule, repeat(seq(',', rule)), optional(','));

module.exports = grammar({
  name: 'buff',

  externals: $ => [
    $._newline, // significant line-end (emitted by external scanner)
    $.indent,   // increase in indentation
    $.dedent,   // decrease in indentation
  ],

  // Whitespace + comments are extras. The external scanner takes priority
  // over /\s/ when one of NEWLINE/INDENT/DEDENT is a valid continuation.
  extras: $ => [
    /[\s]/,
    $.comment,
  ],

  // Resolve ambiguities explicitly (tree-sitter warns if not declared).
  conflicts: $ => [
    [$.identifier_pattern, $.variant_pattern],
    [$.null_conditional_expression],
    [$.if_statement, $.if_expression],
    [$.if_expression],
  ],

  rules: {
    // =====================================================================
    // Top level
    // =====================================================================

    source_file: $ => repeat($._declaration),

    _declaration: $ => choice(
      $.function_declaration,
      $.import_declaration,
      $.export_declaration,
      $.enum_declaration,
      $.trait_declaration,
      $.extend_block,
      $.extern_crate_declaration,
    ),

    // =====================================================================
    // Attributes (`@name` / `@name(args)`)
    // =====================================================================

    attribute: $ => seq(
      '@',
      field('name', $.identifier),
      optional(seq('(', commaSep($.identifier), ')')),
    ),

    // =====================================================================
    // Function declarations
    // =====================================================================

    function_declaration: $ => seq(
      repeat($.attribute),
      optional('unsafe'),
      optional(choice('async', 'extern', seq('extern', 'async'))),
      'func',
      field('name', $.identifier),
      field('parameters', $.parameter_list),
      optional(seq('->', field('return_type', $._type))),
      field('body', $.block),
    ),

    parameter_list: $ => seq('(', commaSep($.parameter), ')'),

    parameter: $ => seq(
      field('name', $.identifier),
      // `self` receiver (no type annotation) — extension-method convention.
      optional(seq(':', field('type', $._type))),
      optional(seq('=', field('default', $._expression))),
    ),

    // =====================================================================
    // Blocks: layout (offside rule) or braces
    // =====================================================================

    block: $ => choice($.layout_block, $.brace_block),

    // Layout-sensitive block: `: INDENT stmt+ DEDENT`.
    // The external scanner's INDENT token implicitly consumes the leading
    // newline + indent whitespace (one token), avoiding the need to
    // sequence NEWLINE -> INDENT across two scanner calls.
    layout_block: $ => seq(
      ':',
      $.indent,
      repeat1($._statement),
      $.dedent,
    ),

    // Brace block (rare for control flow in Buff; used for `func` bodies
    // when the user prefers brace style).
    brace_block: $ => seq(
      '{',
      repeat($._statement),
      '}',
    ),

    // =====================================================================
    // Statements
    // =====================================================================

    _statement: $ => choice(
      $.let_declaration,
      $.if_statement,
      $.for_statement,
      $.while_statement,
      $.match_statement,
      $.return_statement,
      $.break_statement,
      $.continue_statement,
      $.guard_statement,
      $.defer_statement,
      $.expression_statement,
    ),

    let_declaration: $ => seq(
      'let',
      optional('mut'),
      field('pattern', choice($._pattern)),
      optional(seq(':', field('type', $._type))),
      '=',
      field('value', $._expression),
    ),

    if_statement: $ => seq(
      'if',
      $.if_condition,
      repeat(seq(',', $.if_condition)),
      optional(','),
      field('consequence', $.block),
      repeat($.elif_clause),
      optional($.else_clause),
    ),

    if_condition: $ => choice(
      seq('let', field('pattern', $._pattern), '=', field('value', $._expression)),
      field('expression', $._expression),
    ),

    elif_clause: $ => seq(
      'else',
      'if',
      $.if_condition,
      repeat(seq(',', $.if_condition)),
      optional(','),
      field('consequence', $.block),
    ),

    else_clause: $ => seq(
      'else',
      field('body', $.block),
    ),

    for_statement: $ => seq(
      'for',
      choice(
        seq(field('variable', $.identifier), 'in', field('iterator', $._expression)),
        seq('let', field('pattern', $._pattern), '=', field('value', $._expression)),
        field('condition', $._expression),
      ),
      field('body', $.block),
    ),

    // BUG-9: `while cond { body }` — conventional conditional loop. Mirrors
    // the `for cond { body }` condition form (third arm above).
    while_statement: $ => seq(
      'while',
      field('condition', $._expression),
      field('body', $.block),
    ),

    match_statement: $ => seq(
      'match',
      field('scrutinee', $._expression),
      field('body', $.match_body),
    ),

    match_body: $ => seq(
      '{',
      repeat($.match_arm),
      '}',
    ),

    match_arm: $ => seq(
      field('pattern', $._pattern),
      '=>',
      field('value', $._expression),
      optional(','),
    ),

    return_statement: $ => choice(
      seq('return', field('value', $._expression)),
      prec(-1, 'return'),
    ),

    break_statement: $ => 'break',
    continue_statement: $ => 'continue',

    guard_statement: $ => seq(
      'guard',
      $.guard_condition,
      repeat(seq(',', $.guard_condition)),
      optional(','),
      'else',
      field('else_block', $.block),
    ),

    guard_condition: $ => choice(
      seq('let', field('pattern', $._pattern), '=', field('value', $._expression)),
      field('expression', $._expression),
    ),

    defer_statement: $ => seq(
      'defer',
      field('expression', $._expression),
    ),

    expression_statement: $ => $._expression,

    // =====================================================================
    // Enum declarations
    // =====================================================================

    enum_declaration: $ => seq(
      'enum',
      field('name', $.identifier),
      optional($.type_parameters),
      field('body', $.enum_body),
    ),

    type_parameters: $ => seq('<', commaSep($.identifier), '>'),

    enum_body: $ => seq(
      '{',
      repeat($.enum_variant),
      '}',
    ),

    enum_variant: $ => seq(
      field('name', $.identifier),
      optional(seq('(', commaSep1($._type), ')')),
      optional(','),
    ),

    // =====================================================================
    // Trait declarations
    // =====================================================================

    trait_declaration: $ => seq(
      'trait',
      field('name', $.identifier),
      optional(seq(':', commaSep1($._type))),
      field('body', $.trait_body),
    ),

    trait_body: $ => seq(
      '{',
      repeat(choice($.trait_method_required, $.trait_method_default)),
      '}',
    ),

    trait_method_required: $ => seq(
      optional(choice('async', 'extern', seq('extern', 'async'))),
      'func',
      field('name', $.identifier),
      field('parameters', $.parameter_list),
      optional(seq('->', field('return_type', $._type))),
      ';',
    ),

    trait_method_default: $ => seq(
      optional(choice('async', 'extern', seq('extern', 'async'))),
      'func',
      field('name', $.identifier),
      field('parameters', $.parameter_list),
      optional(seq('->', field('return_type', $._type))),
      field('body', $.block),
      optional(';'),
    ),

    // =====================================================================
    // Extend block (extension methods)
    // =====================================================================

    extend_block: $ => seq(
      'extend',
      field('target', $._type),
      '{',
      repeat($.function_declaration),
      '}',
    ),

    // =====================================================================
    // Import / Export / Extern crate
    // =====================================================================

    import_declaration: $ => seq(
      'import',
      choice(
        seq('*', 'from', field('source', $.string)),
        seq('{', commaSep($.identifier), '}', 'from', field('source', $.string)),
        seq(field('default', $.identifier), 'from', field('source', $.string)),
        seq($.identifier, repeat(seq('.', $.identifier)), optional(seq('as', $.identifier))),
      ),
    ),

    export_declaration: $ => seq(
      'export',
      choice(
        $.function_declaration,
        $.enum_declaration,
        seq('*', 'from', field('source', $.string)),
        seq('{', commaSep($.identifier), '}', optional(seq('from', field('source', $.string)))),
      ),
    ),

    extern_crate_declaration: $ => seq(
      'extern',
      'crate',
      field('name', $.string),
    ),

    // =====================================================================
    // Types
    // =====================================================================

    _type: $ => $.type_expression,

    type_expression: $ => choice(
      $.named_type,
      $.generic_type,
      $.tuple_type,
      $.union_type,
    ),

    named_type: $ => choice($.type_identifier, $.identifier),

    generic_type: $ => prec(PREC.postfix, seq(
      field('base', choice($.named_type, $.generic_type)),
      '<',
      commaSep1(field('arg', $._type)),
      '>',
    )),

    tuple_type: $ => seq(
      '(',
      $._type,
      ',',
      commaSep1($._type),
      ')',
    ),

    union_type: $ => prec.left(PREC.bitwise_or, seq(
      field('left', choice($.named_type, $.generic_type, $.tuple_type)),
      '|',
      field('right', choice($.named_type, $.generic_type, $.tuple_type)),
      repeat(seq('|', choice($.named_type, $.generic_type, $.tuple_type))),
    )),

    // =====================================================================
    // Expressions
    // =====================================================================

    _expression: $ => choice(
      $.assignment_expression,
      $.binary_expression,
      $.unary_expression,
      $.range_expression,
      $.call_expression,
      $.method_call_expression,
      $.field_access_expression,
      $.index_expression,
      $.try_expression,
      $.null_conditional_expression,
      $.primary_expression,
    ),

    assignment_expression: $ => prec.right(PREC.assignment, seq(
      field('left', $._expression),
      field('operator', choice('=', '+=', '-=', '*=', '/=', '%=')),
      field('right', $._expression),
    )),

    binary_expression: $ => choice(
      ...[
        ['+', PREC.additive], ['-', PREC.additive],
        ['*', PREC.multiplicative], ['/', PREC.multiplicative], ['%', PREC.multiplicative],
        ['==', PREC.equality], ['!=', PREC.equality],
        ['<', PREC.comparison], ['>', PREC.comparison], ['<=', PREC.comparison], ['>=', PREC.comparison],
        ['&&', PREC.logical_and],
        ['||', PREC.logical_or],
        // BUG-4: word-operator aliases. `and`/`or` share the precedence of
        // their symbolic twins (`&&`/`||`) so highlighting and parsing match
        // the authoritative Rust parser in `crates/buff-lang-parser/`.
        ['and', PREC.logical_and],
        ['or', PREC.logical_or],
        ['&', PREC.bitwise_and],
        ['|', PREC.bitwise_or],
        ['^', PREC.bitwise_xor],
        ['<<', PREC.shift], ['>>', PREC.shift],
        ['|>', PREC.pipeline],
      ].map(([op, precedence]) =>
        prec.left(precedence, seq(
          field('left', $._expression),
          // Trick: use literal-as-field for clarity
          field('operator', op),
          field('right', $._expression),
        )),
      ),
    ),

    null_coalesce_expression: $ => prec.right(PREC.null_coalesce, seq(
      field('left', $._expression),
      field('operator', '??'),
      field('right', $._expression),
    )),

    range_expression: $ => prec.left(PREC.range, seq(
      field('start', $._expression),
      field('operator', choice('..', '..=')),
      field('end', $._expression),
    )),

    unary_expression: $ => prec(PREC.unary, seq(
      // BUG-4: `not` is a word alias for `!` (same precedence, tightest of
      // the three word operators: `not` > `and` > `or`).
      field('operator', choice('-', '!', '~', 'not')),
      field('argument', $._expression),
    )),

    call_expression: $ => prec(PREC.postfix, seq(
      field('function', $._expression),
      field('arguments', $.argument_list),
    )),

    argument_list: $ => seq(
      '(',
      commaSep(choice($.named_argument, $._expression)),
      ')',
    ),

    named_argument: $ => seq(
      field('name', $.identifier),
      ':',
      field('value', $._expression),
    ),

    method_call_expression: $ => prec(PREC.postfix + 1, seq(
      field('receiver', $._expression),
      '.',
      field('method', $.identifier),
      field('arguments', $.argument_list),
    )),

    field_access_expression: $ => prec(PREC.postfix, seq(
      field('object', $._expression),
      '.',
      field('field', $.identifier),
    )),

    index_expression: $ => prec(PREC.postfix, seq(
      field('object', $._expression),
      '[',
      commaSep1($._expression),
      ']',
    )),

    try_expression: $ => prec(PREC.postfix, seq(
      field('argument', $._expression),
      '?',
    )),

    null_conditional_expression: $ => prec(PREC.postfix, seq(
      field('receiver', $._expression),
      '?.',
      field('field', $.identifier),
      optional(field('arguments', $.argument_list)),
    )),

    primary_expression: $ => choice(
      $.literal,
      $.identifier,
      $.parenthesized_expression,
      $.tuple_expression,
      $.array_expression,
      $.map_expression,
      $.closure_expression,
      $.struct_init,
      $.if_expression,
      $.spawn_expression,
    ),

    parenthesized_expression: $ => seq('(', $._expression, ')'),

    tuple_expression: $ => seq(
      '(',
      $._expression,
      ',',
      commaSep1($._expression),
      ')',
    ),

    array_expression: $ => seq(
      '[',
      commaSep($._expression),
      ']',
    ),

    // `{:}` empty map OR `{k: v, ...}` non-empty.
    // Closures `{x => ...}` and struct-init `Type { ... }` disambiguate by shape.
    map_expression: $ => choice(
      seq('{', ':', '}'),
      seq('{', $.map_entry, repeat(seq(',', $.map_entry)), optional(','), '}'),
    ),

    map_entry: $ => seq(
      field('key', $._expression),
      ':',
      field('value', $._expression),
    ),

    // `{ params => body }` — params are bare identifiers (no types).
    closure_expression: $ => seq(
      '{',
      $.identifier,
      repeat(seq(',', $.identifier)),
      '=>',
      field('body', $._expression),
      '}',
    ),

    // `Type { field: value, ... }` — must follow an identifier (the type name).
    // The type identifier is conventionally PascalCase (uppercase first
    // letter) — see `type_identifier` below — so this rule does not collide
    // with `match x { ... }` or `if cond { ... }` whose receiver is a
    // lowercase variable.
    struct_init: $ => prec(PREC.postfix, seq(
      field('type', $.type_identifier),
      '{',
      commaSep($.struct_field),
      '}',
    )),

    struct_field: $ => seq(
      field('name', $.identifier),
      ':',
      field('value', $._expression),
    ),

    if_expression: $ => seq(
      'if',
      $.if_condition,
      repeat(seq(',', $.if_condition)),
      optional(','),
      field('consequence', $.block),
      repeat($.elif_clause),
      optional($.else_clause),
    ),

    spawn_expression: $ => seq(
      'spawn',
      field('task', $._expression),
    ),

    // =====================================================================
    // Patterns (match arms, let-destructuring, if-let, guard-let)
    // =====================================================================

    _pattern: $ => choice(
      $.wildcard_pattern,
      $.identifier_pattern,
      $.variant_pattern,
      $.struct_pattern,
      $.tuple_pattern,
      $.literal_pattern,
      $.negative_int_pattern,
    ),

    wildcard_pattern: $ => '_',

    identifier_pattern: $ => $.identifier,

    variant_pattern: $ => seq(
      field('variant', $.identifier),
      optional(seq('(', commaSep($._pattern), ')')),
    ),

    struct_pattern: $ => seq(
      field('name', $.identifier),
      '{',
      commaSep($.struct_pattern_field),
      '}',
    ),

    struct_pattern_field: $ => seq(
      field('name', $.identifier),
      optional(seq(':', field('pattern', $._pattern))),
    ),

    tuple_pattern: $ => seq(
      '(',
      commaSep($._pattern),
      ')',
    ),

    literal_pattern: $ => $.literal,

    negative_int_pattern: $ => seq('-', $.integer_literal),

    // =====================================================================
    // Literals & leaf tokens
    // =====================================================================

    literal: $ => choice(
      $.integer_literal,
      $.float_literal,
      $.string,
      $.boolean,
      $.char_literal,
    ),

    integer_literal: $ => token(/[0-9][0-9_]*/),

    float_literal: $ => token(/[0-9][0-9_]*\.[0-9][0-9_]*([eE][+-]?[0-9]+)?/),

    boolean: $ => choice('true', 'false'),

    char_literal: $ => token(seq(
      '\'',
      choice(/[^'\\\n]/, /\\./, /\\u\{[0-9a-fA-F]+\}/),
      '\'',
    )),

    // Plain string literal (no interpolation). Buff's lexer tokenizes every
    // `"..."` as StringStart/StringPart/StringEnd; here we collapse to a
    // single node for simplicity. Interpolation is parsed as a child of
    // the string node (the `{` inside is data, not a block).
    string: $ => seq(
      '"',
      repeat(choice(
        $.string_fragment,
        $.escape_sequence,
        $.interpolation,
      )),
      '"',
    ),

    string_fragment: $ => token.immediate(prec(1, /[^"\\\{\n]+/)),

    escape_sequence: $ => token.immediate(seq(
      '\\',
      choice(
        /[^xuU]/,
        /[0-7]{1,3}/,
        /x[0-9a-fA-F]{2}/,
        /u[0-9a-fA-F]{4}/,
        /U[0-9a-fA-F]{8}/,
        /u\{[0-9a-fA-F]+\}/,
      ),
    )),

    interpolation: $ => seq(
      '{',
      $._expression,
      '}',
    ),

    identifier: $ => /[A-Za-z_][A-Za-z0-9_]*/,

    // Convention: types use PascalCase (uppercase first letter). This is the
    // standard Rust/Buff naming convention and lets the grammar cleanly
    // disambiguate `Foo { ... }` (struct init) from `if c { ... }` or
    // `match x { ... }` (block bodies) by lexical means.
    type_identifier: $ => /[A-Z][A-Za-z0-9_]*/,

    comment: $ => choice(
      token(seq('//', /.*/)),
      token(seq('/*', /[^*]*\*+([^/*][^*]*\*+)*/, '/')),
    ),
  },
});
