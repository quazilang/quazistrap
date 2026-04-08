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
        args: Vec<Expr>,
    },

    Field {
        object: Box<Expr>,
        name: String,
    },

    MethodCall {
        object: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },
}

pub type Expr = Spanned<ExprKind>;

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
    Named(String),
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
    pub params: Vec<Type>,
    pub return_ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ItemKind {
    Fn {
        name: String,
        params: Vec<(String, Type)>,
        return_ty: Type,
        body: Block,
    },
    Struct {
        name: String,
        fields: Vec<(String, Type, bool)>, // (name, type, const?)
    },
    Trait {
        name: String,
        methods: Vec<TraitMethod>,
    },
    Impl {
        trait_name: String,
        for_type: String,
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
