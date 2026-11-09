# Lexical structure

Audience: language users and implementers.

## Source encoding

Quazi source files use the `.qz` extension and are encoded in UTF-8. The
compiler rejects invalid UTF-8 byte sequences during lexing.

## Comments

Line comments begin with `//` and extend to the end of the line. Block
comments are not supported.

```quazi
// This is a line comment.
const x: i32 = 42; // Trailing comment.
```

## Identifiers

Identifiers begin with a letter or underscore, followed by letters, digits, or
underscores. Module and symbol paths use `.`, never `::`.

```quazi
const my_value: i32 = 0;
import std.io.stdout;
```

## Keywords

The following identifiers are reserved keywords:

`as`, `break`, `const`, `continue`, `dyn`, `else`, `enum`, `false`, `fn`,
`for`, `if`, `impl`, `import`, `match`, `mod`, `pub`, `qo`, `ret`, `self`,
`struct`, `trait`, `true`, `type`, `unsafe`, `var`.

`any` is reserved syntax for the compiler-erased `@format` pseudo-parameter
and is rejected in user declarations.

## Literals

### Integer literals

Decimal integer literals consist of one or more decimal digits. Leading zeros
are permitted. Negative integers use the unary `-` operator.

```quazi
const zero: i32 = 0;
const answer: i32 = 42;
const big: u64 = 18446744073709551615;
```

Hexadecimal literals use the `0x` prefix:

```quazi
const mask: u32 = 0xFF;
const flags: u32 = 0x0F;
```

### Floating-point literals

Floating-point literals contain a decimal point, an exponent, or both:

```quazi
const pi: f64 = 3.14159;
const avogadro: f64 = 6.022e23;
const small: f64 = 1.5e-10;
```

### Boolean literals

```quazi
const yes: bool = true;
const no: bool = false;
```

### String literals

Quoted strings use double quotes and decode escape sequences:

```quazi
const greeting: str = "Hello, world!\n";
const path: str = "C:\\data\\file.txt";
```

Every quoted string is a format string ([D-020](../decisions/string-interpolation.md)):
`{expr}` interpolates a value (with an optional `:spec` such as `{x:X}`) and
the literal becomes an owned `String`; `{{` and `}}` write single braces. A
literal without placeholders stays a `str`. An unmatched brace, or an empty
`{}` outside a format call's template, is an error.

```quazi
const name: str = "quazi";
const line = "hello {name}, answer {6 * 7}";   // String
const json: str = "{{\"a\": 1}}";             // {"a": 1}
io.println("x = {}, y = {y}", x);             // format call template
```

In a format call (`println`, `format`, ...) the first literal is the template:
`{}` and `{:spec}` there consume the explicit arguments in order.

Raw backtick strings preserve contents exactly, with no escape decoding and no
interpolation (braces are literal). They may span multiple lines and must be
terminated:

```quazi
const raw: str = `C:\data\file.txt`;
const multi: str = `line one
line two`;
```

### Byte string literals

`b"..."` decodes byte escapes into an immutable `bytes` value. `br"..."` is
the raw byte-string form that preserves contents exactly:

```quazi
const header: bytes = b"PNG\x0D\x0A";
const raw_bytes: bytes = br"\x00 is text here";
```

## Escape sequences

Quoted strings (not raw strings) decode the following escape sequences:

| Escape | Meaning |
| --- | --- |
| `\0` | Null byte |
| `\a` | Bell (BEL) |
| `\b` | Backspace |
| `\e` | ANSI escape (ESC, 0x1B) |
| `\f` | Form feed |
| `\n` | Line feed |
| `\r` | Carriage return |
| `\t` | Horizontal tab |
| `\v` | Vertical tab |
| `\\` | Backslash |
| `\"` | Double quote |
| `\'` | Single quote |
| `\xNN` | Hexadecimal ASCII byte (two hex digits) |
| `\NNN` | Octal byte (one to three octal digits) |
| `\uNNNN` | C-style Unicode scalar (exactly four hex digits) |
| `\UNNNNNNNN` | C-style Unicode scalar (exactly eight hex digits) |
| `\u{H...}` | Rust-style Unicode scalar (one to six hex digits) |
| `\` + newline | Line continuation (newline is consumed) |

Invalid escape sequences produce a lexer error. Surrogate codepoints and
values above U+10FFFF are rejected in Unicode escapes.

## Separators and punctuation

Statements end with `;`. Braces `{}` delimit blocks. Parentheses `()` group
expressions and parameter lists. Brackets `[]` delimit generic arguments,
array types, and index expressions. Commas `,` separate items in lists.
Colons `:` separate identifiers from types, loop variables from iterables, and
match arms from results. The arrow `=>` separates match patterns from bodies.
The double-dot `..` and `..=` form range expressions.

## Operator tokens

Arithmetic: `+`, `-`, `*`, `/`, `%`, `**`.
Assignment: `=`, `+=`, `-=`, `*=`, `/=`, `%=`.
Increment/decrement: `++`, `--`.
Comparison: `==`, `!=`, `<`, `<=`, `>`, `>=`.
Logical: `&&`, `||`, `!`.
Bitwise: `&`, `|`, `^`, `<<`, `>>`.
Reference/dereference: `&` (address-of), `*` (dereference).
Cast: `as`.
Error propagation: `?`.
