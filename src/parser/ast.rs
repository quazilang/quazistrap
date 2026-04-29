// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub col: usize,
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(line: usize, col: usize, start: usize, end: usize) -> Self {
        Self {
            line,
            col,
            start,
            end,
        }
    }

    pub fn merge(a: Span, b: Span) -> Self {
        let (line, col, start) = if a.start <= b.start {
            (a.line, a.col, a.start)
        } else {
            (b.line, b.col, b.start)
        };
        let end = a.end.max(b.end);
        Self {
            line,
            col,
            start,
            end,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Self { node, span }
    }
}

#[derive(Debug, Clone)]
pub enum Literal {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
}

#[derive(Debug, Clone)]
pub enum UnaryOpKind {
    Neg, // -x
    Not, // !x
}

#[derive(Debug, Clone)]
pub enum BinOpKind {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Lt,
    Gt,
    LtEq,
    GtEq,
    EqEq,
    NotEq,
    AndAnd,
    OrOr,
}

#[derive(Debug, Clone)]
pub enum CompoundAssignOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Debug, Clone)]
pub enum IncDecOp {
    Inc,
    Dec,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Literal(Literal),
    Ident(String),
    Group(Box<Expr>),

    Unary {
        op: UnaryOpKind,
        expr: Box<Expr>,
    },

    Binary {
        left: Box<Expr>,
        op: BinOpKind,
        right: Box<Expr>,
    },

    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
    },

    Call {
        callee: Box<Expr>,
        type_args: Vec<Type>,
        args: Vec<Expr>,
    },

    Field {
        object: Box<Expr>,
        name: String,
    },

    MethodCall {
        object: Box<Expr>,
        method: String,
        type_args: Vec<Type>,
        args: Vec<Expr>,
    },

    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
    },

    CompoundAssign {
        target: Box<Expr>,
        op: CompoundAssignOp,
        value: Box<Expr>,
    },

    IncDec {
        expr: Box<Expr>,
        op: IncDecOp,
        prefix: bool,
    },
}

pub type Expr = Spanned<ExprKind>;

#[derive(Debug, Clone)]
pub enum PatternKind {
    Wildcard,
    Variant {
        enum_name: Option<String>,
        variant: String,
        bindings: Vec<String>,
    },
}

pub type Pattern = Spanned<PatternKind>;

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub expr: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypeKind {
    Int8,
    Int16,
    Int32,
    Int64,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Float16,
    Float32,
    Float64,
    Bool,
    Str,
    Void,
    Any,
    Named {
        name: String,
        type_args: Vec<Type>,
    },
}

impl std::fmt::Display for TypeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeKind::Int8 => write!(f, "int8"),
            TypeKind::Int16 => write!(f, "int16"),
            TypeKind::Int32 => write!(f, "int32"),
            TypeKind::Int64 => write!(f, "int64"),
            TypeKind::Uint8 => write!(f, "uint8"),
            TypeKind::Uint16 => write!(f, "uint16"),
            TypeKind::Uint32 => write!(f, "uint32"),
            TypeKind::Uint64 => write!(f, "uint64"),
            TypeKind::Float16 => write!(f, "float16"),
            TypeKind::Float32 => write!(f, "float32"),
            TypeKind::Float64 => write!(f, "float64"),
            TypeKind::Bool => write!(f, "bool"),
            TypeKind::Str => write!(f, "str"),
            TypeKind::Void => write!(f, "void"),
            TypeKind::Any => write!(f, "any"),
            TypeKind::Named { name, type_args } => {
                if type_args.is_empty() {
                    write!(f, "{}", name)
                } else {
                    write!(f, "{}[", name)?;
                    for (i, arg) in type_args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", arg.node)?;
                    }
                    write!(f, "]")
                }
            }
        }
    }
}

pub type Type = Spanned<TypeKind>;

#[derive(Debug, Clone)]
pub enum StmtKind {
    Var {
        name: String,
        ty: Option<Type>,
        value: Option<Expr>,
    },
    Const {
        name: String,
        ty: Option<Type>,
        value: Expr,
    },
    Return(Option<Expr>),
    If {
        condition: Expr,
        then_block: Block,
        else_block: Option<Block>,
    },
    While {
        condition: Expr,
        body: Block,
    },
    ExprStmt(Expr),
}

pub type Stmt = Spanned<StmtKind>;

#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ImportPath {
    pub path: Vec<String>,
    pub items: ImportItems,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ImportItems {
    Single(String),
    Multiple(Vec<String>),
    Aliased(String, String),
    All,
}

#[derive(Debug, Clone)]
pub struct TraitMethod {
    pub name: String,
    pub generic_params: Vec<String>,
    pub params: Vec<Type>,
    pub return_ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub payload_types: Vec<Type>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ItemKind {
    Fn {
        name: String,
        generic_params: Vec<String>,
        params: Vec<(String, Type)>,
        return_ty: Type,
        body: Block,
    },
    Struct {
        name: String,
        generic_params: Vec<String>,
        fields: Vec<(String, Type, bool)>, // (name, type, const?)
    },
    Trait {
        name: String,
        generic_params: Vec<String>,
        methods: Vec<TraitMethod>,
    },
    Enum {
        name: String,
        generic_params: Vec<String>,
        variants: Vec<EnumVariant>,
    },
    Impl {
        trait_ty: Type,
        for_ty: Type,
        methods: Vec<Item>,
    },
    Import(ImportPath),
}

pub type Item = Spanned<ItemKind>;

#[derive(Debug, Clone)]
pub struct Program {
    pub items: Vec<Item>,
    pub span: Option<Span>,
}
