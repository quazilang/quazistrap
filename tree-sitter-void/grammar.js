// tree-sitter grammar for the Void programming language
// Run `tree-sitter generate` to produce src/parser.c

module.exports = grammar({
  name: 'void',

  extras: $ => [/\s+/, $.comment],

  word: $ => $.identifier,

  conflicts: $ => [
    [$._expr, $.named_type],
    [$.match_stmt, $.match_expr],
  ],

  rules: {
    source_file: $ => repeat($._item),

    comment: $ => token(seq('//', /.*/)),

    // ── Top-level items ───────────────────────────────────────────────────────

    _item: $ => choice(
      $.import_decl,
      $.fn_decl,
      $.struct_decl,
      $.trait_decl,
      $.enum_decl,
      $.impl_decl,
    ),

    // import a.b.{x, y};  /  import a.b.*;  /  import a.b as c;  /  import a.b;
    import_decl: $ => seq(
      'import',
      $.import_path,
      optional(choice(
        seq('.', $.glob_import),
        seq('.', $.multi_import),
        seq('as', $.identifier),
      )),
      ';',
    ),
    import_path: $ => prec.left(seq($.identifier, repeat(seq('.', $.identifier)))),
    glob_import: $ => '*',
    multi_import: $ => seq('{', commaSep1($.identifier), '}'),

    // ── Attributes ────────────────────────────────────────────────────────────

    attribute: $ => seq(
      '@',
      field('name', $.identifier),
      optional(seq('(', commaSep($.attr_arg), ')')),
    ),

    attr_arg: $ => choice(
      seq($.identifier, '=', $._attr_value),
      $._attr_value,
    ),

    _attr_value: $ => choice($.string_literal, $.integer_literal, $.identifier),

    // ── Function declaration ─────────────────────────────────────────────────

    fn_decl: $ => seq(
      repeat($.attribute),
      optional('pub'),
      optional('unsafe'),
      'fn',
      field('name', $.identifier),
      optional($.generic_params),
      '(',
      commaSep($.param),
      ')',
      field('return_type', $._type),
      choice($.block, ';'),
    ),

    generic_params: $ => seq('[', commaSep1($.identifier), ']'),

    param: $ => seq(
      repeat($.attribute),
      field('name', $.identifier),
      ':',
      field('type', $._type),
      optional('...'),
    ),

    // ── Struct ────────────────────────────────────────────────────────────────

    struct_decl: $ => seq(
      repeat($.attribute),
      optional('pub'),
      'struct',
      field('name', $.identifier),
      optional($.generic_params),
      '{',
      commaSep($.struct_field),
      optional(','),
      '}',
    ),

    struct_field: $ => seq(
      optional('const'),
      field('name', $.identifier),
      ':',
      field('type', $._type),
    ),

    // ── Trait ─────────────────────────────────────────────────────────────────

    trait_decl: $ => seq(
      repeat($.attribute),
      optional('pub'),
      'trait',
      field('name', $.identifier),
      optional($.generic_params),
      '{',
      repeat($.trait_method),
      '}',
    ),

    trait_method: $ => seq(
      'fn',
      field('name', $.identifier),
      optional($.generic_params),
      '(',
      commaSep($._type),
      ')',
      $._type,
      ';',
    ),

    // ── Enum ──────────────────────────────────────────────────────────────────

    enum_decl: $ => seq(
      repeat($.attribute),
      optional('pub'),
      'enum',
      field('name', $.identifier),
      optional($.generic_params),
      '{',
      commaSep($.enum_variant),
      optional(','),
      '}',
    ),

    enum_variant: $ => seq(
      field('name', $.identifier),
      optional(seq('(', commaSep($._type), ')')),
    ),

    // ── Impl ──────────────────────────────────────────────────────────────────

    impl_decl: $ => seq(
      'impl',
      field('trait_name', $._type),
      'for',
      field('struct_name', $._type),
      '{',
      repeat($.fn_decl),
      '}',
    ),

    // ── Statements ────────────────────────────────────────────────────────────

    block: $ => seq('{', repeat($._stmt), '}'),

    _stmt: $ => choice(
      $.var_stmt,
      $.const_stmt,
      $.return_stmt,
      $.if_stmt,
      $.for_stmt,
      $.while_stmt,
      $.match_stmt,
      $.cfg_block,
      $.unsafe_block,
      $.expr_stmt,
    ),

    var_stmt: $ => seq(
      repeat($.attribute),
      'var',
      field('name', $.identifier),
      optional(seq(':', field('type', $._type))),
      optional(seq('=', field('value', $._expr))),
      ';',
    ),

    const_stmt: $ => seq(
      repeat($.attribute),
      'const',
      field('name', $.identifier),
      optional(seq(':', field('type', $._type))),
      '=',
      field('value', $._expr),
      ';',
    ),

    return_stmt: $ => seq('ret', optional($._expr), ';'),

    if_stmt: $ => seq(
      'if',
      '(',
      field('condition', $._expr),
      ')',
      field('then', $.block),
      optional(seq('else', choice($.block, $.if_stmt))),
    ),

    for_stmt: $ => seq(
      'for',
      choice(
        seq(
          field('var', $.identifier),
          optional(seq(',', field('idx', $.identifier))),
          ':',
          field('iter', $._expr),
          optional(seq('..', field('end', $._expr))),
        ),
        field('condition', $._expr),
      ),
      field('body', $.block),
    ),

    while_stmt: $ => seq(
      'while',
      '(',
      field('condition', $._expr),
      ')',
      field('body', $.block),
    ),

    match_stmt: $ => seq(
      'match',
      field('value', $._expr),
      '{',
      commaSep($.match_arm),
      optional(','),
      '}',
    ),

    match_arm: $ => seq(
      field('pattern', $.match_pattern),
      '=>',
      field('body', $._expr),
    ),

    match_pattern: $ => choice(
      $.wildcard_pattern,
      $.identifier,
      $.qualified_pattern,
      $.tuple_pattern,
    ),

    wildcard_pattern: $ => '_',
    qualified_pattern: $ => seq($.identifier, '.', $.identifier),
    tuple_pattern: $ => seq($.identifier, '(', commaSep($.identifier), ')'),

    cfg_block: $ => seq(
      '@cfg',
      '(',
      $.attr_arg,
      ')',
      $.block,
    ),

    unsafe_block: $ => seq('unsafe', $.block),

    expr_stmt: $ => seq($._expr, ';'),

    // ── Types ─────────────────────────────────────────────────────────────────

    _type: $ => choice(
      $._primitive_type,
      $.named_type,
      $.ref_type,
      $.ptr_type,
      $.array_type,
      $.slice_type,
      $.never_type,
    ),

    _primitive_type: $ => choice(
      'i8', 'i16', 'i32', 'i64',
      'u8', 'u16', 'u32', 'u64',
      'isize', 'usize',
      'f16', 'f32', 'f64',
      'bool', 'str', 'void', 'any',
    ),

    named_type: $ => seq(
      $.identifier,
      optional(seq('[', commaSep1($._type), ']')),
    ),

    ref_type: $ => seq('&', $._type),
    ptr_type: $ => seq('*', $._type),
    array_type: $ => seq('[', $._type, ';', $.integer_literal, ']'),
    slice_type: $ => seq('[', $._type, ']'),
    never_type: $ => '!',

    // ── Expressions (precedence climbing) ─────────────────────────────────────

    _expr: $ => choice(
      $.assign_expr,
      $.compound_assign_expr,
      $.binary_expr,
      $.unary_expr,
      $.postfix_expr,
      $.call_expr,
      $.method_call_expr,
      $.field_expr,
      $.index_expr,
      $.match_expr,
      $.array_lit,
      $.range_expr,
      $.identifier,
      $.integer_literal,
      $.float_literal,
      $.string_literal,
      $.bool_literal,
      $.paren_expr,
    ),

    assign_expr: $ => prec.right(1, seq(
      field('lhs', $._expr),
      '=',
      field('rhs', $._expr),
    )),

    compound_assign_expr: $ => prec.right(1, seq(
      field('lhs', $._expr),
      field('op', choice('+=', '-=', '*=', '/=', '%=')),
      field('rhs', $._expr),
    )),

    binary_expr: $ => {
      const table = [
        [prec.left, 2,  '||'],
        [prec.left, 3,  '&&'],
        [prec.left, 4,  choice('==', '!=')],
        [prec.left, 5,  choice('<', '<=', '>', '>=')],
        [prec.left, 6,  choice('+', '-')],
        [prec.left, 7,  choice('*', '/', '%')],
        [prec.right, 8, '**'],
      ];
      return choice(...table.map(([assoc, prec_n, op]) =>
        assoc(prec_n, seq(
          field('lhs', $._expr),
          field('op', op),
          field('rhs', $._expr),
        ))
      ));
    },

    unary_expr: $ => prec(9, seq(
      field('op', choice('-', '!', '++', '--')),
      field('operand', $._expr),
    )),

    postfix_expr: $ => prec.left(10, seq(
      field('operand', $._expr),
      field('op', choice('++', '--')),
    )),

    call_expr: $ => prec.left(10, seq(
      field('callee', $._expr),
      optional(seq('[', commaSep1($._type), ']')),
      '(',
      commaSep($._expr),
      ')',
    )),

    method_call_expr: $ => prec.left(10, seq(
      field('receiver', $._expr),
      '.',
      field('method', $.identifier),
      optional(seq('[', commaSep1($._type), ']')),
      '(',
      commaSep($._expr),
      ')',
    )),

    field_expr: $ => prec.left(10, seq(
      field('object', $._expr),
      '.',
      field('field', $.identifier),
    )),

    index_expr: $ => prec.left(10, seq(
      field('object', $._expr),
      '[',
      field('index', $._expr),
      ']',
    )),

    match_expr: $ => seq(
      'match',
      field('value', $._expr),
      '{',
      commaSep($.match_arm),
      optional(','),
      '}',
    ),

    array_lit: $ => seq('[', commaSep($._expr), ']'),

    range_expr: $ => prec.left(5, seq(
      field('start', $._expr),
      '..',
      field('end', $._expr),
    )),

    paren_expr: $ => seq('(', $._expr, ')'),

    // ── Terminals ─────────────────────────────────────────────────────────────

    identifier: $ => /[a-zA-Z_][a-zA-Z0-9_]*/,
    integer_literal: $ => /[0-9]+/,
    float_literal: $ => /[0-9]+\.[0-9]+([eE][+-]?[0-9]+)?/,
    string_literal: $ => /"([^"\\]|\\.)*"/,
    bool_literal: $ => choice('true', 'false'),
  },
});

// ── Helpers ───────────────────────────────────────────────────────────────────

function commaSep(rule) {
  return optional(commaSep1(rule));
}

function commaSep1(rule) {
  return seq(rule, repeat(seq(',', rule)));
}

