// void - the programming language
// Copyright titago (C) 2026
// SPDX-License-Identifier: 0BSD

#[derive(Debug, Clone)]
/// expressions: compares, method calls, literals
pub enum Expr {
    IntLit(i64),       // 5
    FloatLit(f64),     // 3.14159
    StringLit(String), // "hello"
    Ident(String),     // x, stdout
    MethodCall {
        // stdout.println(...)
        object: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },
    BinOp {
        // a + b ; x > y
        left: Box<Expr>,
        op: BinOpKind,
        right: Box<Expr>,
    },
}

/// operator kinds like adding, comparing
#[derive(Debug, Clone)]
pub enum BinOpKind {
    Add,
    Sub,
    Mul,
    Div,
    Lt,
    Gt,
    LtEq,
    GtEq,
    EqEq,
    NotEq,
}

#[derive(Debug, Clone)]
pub struct ImportPath {
    pub path: Vec<String>,
    pub items: ImportItems,
}

#[derive(Debug, Clone)]
pub enum ImportItems {
    Single(String),
    Multiple(Vec<String>),
    Aliased(String, String),
    All,
}

/// language types
#[derive(Debug, Clone)]
pub enum Type {
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
    Named(String), // structs
}

/// statements
#[derive(Debug, Clone)]
pub enum Stmt {
    Let {
        // let x: int32 = 5
        name: String,
        mutable: bool,
        ty: Option<Type>,
        value: Option<Expr>,
    },
    Return(Option<Expr>), // return x
    If {
        // if (x > y) { ... } else { ... }
        condition: Expr,
        then_block: Vec<Stmt>,
        else_block: Option<Vec<Stmt>>,
    },
    // stdout.println(...)
    ExprStmt(Expr),
}

/// top-level statements
#[derive(Debug, Clone)]
pub enum Item {
    // fn add(a: int32, b: int32) int32 { ... }
    Fn {
        name: String,
        params: Vec<(String, Type)>,
        return_ty: Type,
        body: Vec<Stmt>,
    },
    // struct Fox { ... }
    Struct {
        name: String,
        fields: Vec<(String, Type, bool)>, // (name, type, const?)
    },
    // trait Animal { ... }
    Trait {
        name: String,
        methods: Vec<TraitMethod>,
    },
    // impl Animal for Fox { ... }
    Impl {
        trait_name: String,
        for_type: String,
        methods: Vec<Item>,
    },
    Import(ImportPath),
}

#[derive(Debug, Clone)]
pub struct TraitMethod {
    pub name: String,
    pub params: Vec<Type>,
    pub return_ty: Type,
}

#[derive(Debug, Clone)]
pub struct Program {
    pub items: Vec<Item>,
}
