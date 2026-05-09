; Keywords
[
  "fn" "struct" "trait" "enum" "impl" "import"
  "for" "while" "if" "else" "match" "ret"
  "const" "var" "pub" "unsafe" "as" "for"
] @keyword

; Control flow keywords get a more specific scope
["ret" "if" "else" "for" "while" "match"] @keyword.control

; Primitive types
[
  "i8" "i16" "i32" "i64"
  "u8" "u16" "u32" "u64"
  "isize" "usize"
  "f16" "f32" "f64"
  "bool" "str" "void" "any"
] @type.builtin

; Function declarations
(fn_decl name: (identifier) @function)

; Type declarations
(struct_decl name: (identifier) @type)
(trait_decl  name: (identifier) @type)
(enum_decl   name: (identifier) @type)
(impl_decl   trait_name: (named_type (identifier) @type))
(impl_decl   struct_name: (named_type (identifier) @type))

; Enum variants
(enum_variant name: (identifier) @constant)

; Attributes
(attribute name: (identifier) @attribute)
"@" @punctuation.special

; Parameters
(param name: (identifier) @variable.parameter)

; Struct fields
(struct_field name: (identifier) @property)

; Function calls
(call_expr callee: (identifier) @function.call)
(method_call_expr method: (identifier) @function.method)

; Field access
(field_expr field: (identifier) @property)

; Literals
(integer_literal) @number
(float_literal)   @number.float
(string_literal)  @string
(bool_literal)    @boolean

; Comments
(comment) @comment

; Operators
[
  "+" "-" "*" "/" "%" "**"
  "=" "==" "!=" "<" "<=" ">" ">="
  "+=" "-=" "*=" "/=" "%="
  "++" "--"
  "&&" "||" "!"
  "=>" ".."
] @operator

; Punctuation
["," ";" ":"] @punctuation.delimiter
["{" "}" "[" "]" "(" ")"] @punctuation.bracket
