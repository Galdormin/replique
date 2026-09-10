mod lexer;
pub(crate) mod pratt;

use crate::parser::{Spanned, ast::Value};

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Var {
        name: String,
    },
    Function {
        name: Spanned<String>,
        args: Vec<Spanned<Expr>>,
    },
    Litteral {
        value: Value,
    },
    Unary {
        op: Spanned<UnaryOp>,
        rhs: Box<Spanned<Expr>>,
    },
    Binary {
        op: Spanned<BinaryOp>,
        lhs: Box<Spanned<Expr>>,
        rhs: Box<Spanned<Expr>>,
    },
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Neg,
}

impl std::fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Not => "not",
            Self::Neg => "-",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
}

impl std::fmt::Display for BinaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Or => "or",
            Self::And => "and",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
        })
    }
}
