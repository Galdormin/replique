mod lexer;
pub(crate) mod pratt;

use std::fmt;

use crate::parser::{
    Spanned,
    ast::Value,
    diagnostic::{DiagnosticKind, Diagnostics, Label},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValueType {
    Bool,
    Float,
    Int,
    String,
    Dict,
    /// Used for [Expr::Var] and [Expr::Function] which are unknown at compilation
    Unknown,
}

impl ValueType {
    pub fn is_number(&self) -> bool {
        matches!(self, ValueType::Float | ValueType::Int)
    }

    pub fn type_of(value: &Value) -> Self {
        match value {
            Value::Bool(_) => Self::Bool,
            Value::String(_) => Self::String,
            Value::Float(_) => Self::Float,
            Value::Int(_) => Self::Int,
        }
    }

    /// Whether two values can be compared: the same type, two numbers, or
    /// anything at all as soon as one side is only known at run time.
    pub fn compatible_with(self, other: &Self) -> bool {
        use ValueType::*;

        self == Unknown
            || *other == Unknown
            || self == *other
            || self.is_number() && other.is_number()
    }
}

impl fmt::Display for ValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Bool => "bool",
            Self::Float => "float",
            Self::Int => "int",
            Self::String => "string",
            Self::Dict => "dict",
            Self::Unknown => "unknown",
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Var {
        name: String,
    },
    Function {
        name: Spanned<String>,
        args: Vec<Spanned<Expr>>,
    },
    Attr {
        base: Box<Spanned<Expr>>,
        attrs: Vec<Spanned<String>>,
    },
    Litteral {
        value: Value,
    },
    /// To represent `{name: Alan, hp: $base + 2}` we need HashMap of [`Expr`]
    LitteralDict(Vec<(Spanned<String>, Spanned<Expr>)>),
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

impl Expr {
    /// Checks every operator of the expression against the types it accepts,
    /// reporting each one that does not fit.
    pub(crate) fn is_type_valid(&self, diags: &mut Diagnostics) -> bool {
        match self {
            Expr::Var { .. } | Expr::Litteral { .. } | Expr::Error => true,
            Expr::Attr { base, .. } => base.value.is_type_valid(diags),
            Expr::LitteralDict(attrs) => {
                let mut valid = true;
                for (_, attr) in attrs {
                    valid &= attr.value.is_type_valid(diags);
                }
                valid
            }
            Expr::Function { args, .. } => {
                // Every argument is checked, and not just up to the first
                // faulty one, so a call is reported in a single pass.
                let mut valid = true;
                for arg in args {
                    valid &= arg.value.is_type_valid(diags);
                }
                valid
            }
            Expr::Unary { op, rhs } => {
                if !rhs.value.is_type_valid(diags) {
                    return false;
                }

                let rhs_type = rhs.value.value_type().expect("checked just above");
                if op.value.type_of(&rhs_type).is_some() {
                    return true;
                }

                diags.push_labeled(
                    op.span.join(rhs.span),
                    DiagnosticKind::InvalidUnaryOperand {
                        op: op.value.to_string(),
                        expected: op.value.expects().to_owned(),
                        received: rhs_type.to_string(),
                    },
                    vec![Label {
                        span: rhs.span,
                        message: rhs_type.to_string(),
                    }],
                );
                false
            }
            Expr::Binary { op, lhs, rhs } => {
                let mut valid = lhs.value.is_type_valid(diags);
                valid &= rhs.value.is_type_valid(diags);
                if !valid {
                    return false;
                }

                let lhs_type = lhs.value.value_type().expect("checked just above");
                let rhs_type = rhs.value.value_type().expect("checked just above");
                if op.value.type_of(&lhs_type, &rhs_type).is_some() {
                    return true;
                }

                diags.push_labeled(
                    lhs.span.join(rhs.span),
                    DiagnosticKind::InvalidBinaryOperands {
                        op: op.value.to_string(),
                        lhs: lhs_type.to_string(),
                        rhs: rhs_type.to_string(),
                    },
                    vec![
                        Label {
                            span: lhs.span,
                            message: lhs_type.to_string(),
                        },
                        Label {
                            span: rhs.span,
                            message: rhs_type.to_string(),
                        },
                    ],
                );
                false
            }
        }
    }

    /// Type the expression evaluates to, or `None` if an operator is applied
    /// to something it does not accept.
    ///
    /// [`ValueType::Unknown`] is an answer, not a failure: it says the type
    /// depends on what the host puts in a variable or gives back from a call.
    pub(crate) fn value_type(&self) -> Option<ValueType> {
        match self {
            Expr::Var { .. } | Expr::Function { .. } | Expr::Attr { .. } | Expr::Error => {
                Some(ValueType::Unknown)
            }
            Expr::Litteral { value } => Some(ValueType::type_of(value)),
            Expr::LitteralDict(_) => Some(ValueType::Dict),
            Expr::Unary { op, rhs } => op.value.type_of(&rhs.value.value_type()?),
            Expr::Binary { op, lhs, rhs } => op
                .value
                .type_of(&lhs.value.value_type()?, &rhs.value.value_type()?),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Neg,
}

impl UnaryOp {
    /// Type this operator gives back for that operand, or `None` if it does
    /// not accept it.
    fn type_of(&self, rhs: &ValueType) -> Option<ValueType> {
        match (self, rhs) {
            // `not` gives a bool whatever happens, so an unknown operand does
            // not make the whole expression unknown.
            (UnaryOp::Not, ValueType::Bool | ValueType::Unknown) => Some(ValueType::Bool),
            (UnaryOp::Neg, ValueType::Unknown) => Some(ValueType::Unknown),
            (UnaryOp::Neg, v) if v.is_number() => Some(*v),
            _ => None,
        }
    }

    /// What this operator does accept, for the diagnostic.
    fn expects(&self) -> &'static str {
        match self {
            UnaryOp::Not => "bool",
            UnaryOp::Neg => "int or float",
        }
    }
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

impl BinaryOp {
    /// Type this operator gives back for those two operands, or `None` if it
    /// does not accept them.
    fn type_of(&self, lhs: &ValueType, rhs: &ValueType) -> Option<ValueType> {
        use BinaryOp::*;
        use ValueType::*;

        // An operator that always gives a bool keeps saying so, even when an
        // operand is only known at run time.
        if matches!(self, Or | And | Eq | Ne | Lt | Le | Gt | Ge) {
            let known = match self {
                Or | And => lhs.compatible_with(&Bool) && rhs.compatible_with(&Bool),
                Eq | Ne => lhs.compatible_with(rhs),
                _ => lhs.compatible_with(&Int) && rhs.compatible_with(&Int),
            };
            return known.then_some(Bool);
        }

        if *lhs == Unknown || *rhs == Unknown {
            return Some(Unknown);
        }

        match self {
            Add | Sub | Mul if lhs.is_number() && rhs.is_number() => {
                Some(if *lhs == Int && *rhs == Int {
                    Int
                } else {
                    Float
                })
            }
            // A division never gives an int back: `7 / 2` is `3.5`.
            Div if lhs.is_number() && rhs.is_number() => Some(Float),
            _ => None,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::expr::pratt::ExprParser;

    /// Type of `src` and the codes of what the check found. The two always
    /// agree: an expression is valid exactly when nothing is reported.
    fn checked(src: &str) -> (Option<ValueType>, Vec<&'static str>) {
        let mut diags = Diagnostics::from_src(src);
        let expr = ExprParser::new(&Spanned::from_text(src, 0), &mut diags).expr(0);
        assert_eq!(diags.errors(), 0, "{src:?} must parse before it is checked");

        let valid = expr.value.is_type_valid(&mut diags);
        let codes: Vec<_> = diags.iter().map(|d| d.kind.code()).collect();
        assert_eq!(valid, codes.is_empty(), "{src:?}");

        (expr.value.value_type(), codes)
    }

    fn vtype(src: &str) -> Option<ValueType> {
        checked(src).0
    }

    fn codes(src: &str) -> Vec<&'static str> {
        checked(src).1
    }

    #[test]
    fn a_literal_has_the_type_it_is_written_with() {
        assert_eq!(vtype("12"), Some(ValueType::Int));
        assert_eq!(vtype("0.5"), Some(ValueType::Float));
        assert_eq!(vtype("true"), Some(ValueType::Bool));
        assert_eq!(vtype(r#""Alice""#), Some(ValueType::String));
    }

    #[test]
    fn arithmetic_keeps_the_int_and_promotes_to_the_float() {
        assert_eq!(vtype("1 + 2"), Some(ValueType::Int));
        assert_eq!(vtype("1 - 2 * 3"), Some(ValueType::Int));
        assert_eq!(vtype("1 + 0.5"), Some(ValueType::Float));
    }

    #[test]
    fn a_division_is_a_float() {
        assert_eq!(vtype("7 / 2"), Some(ValueType::Float));
    }

    /// Arithmetic is for numbers only, `+` included: a string never takes
    /// part in it, and the VM refuses the same thing at run time.
    #[test]
    fn no_operator_joins_two_strings() {
        assert_eq!(codes(r#""a" + "b""#), ["invalid-binary-operands"]);
        assert_eq!(codes(r#""a" - "b""#), ["invalid-binary-operands"]);
    }

    #[test]
    fn a_comparison_is_a_bool() {
        assert_eq!(vtype("1 < 2"), Some(ValueType::Bool));
        assert_eq!(vtype("1 == 0.5"), Some(ValueType::Bool));
        assert_eq!(vtype("true and false"), Some(ValueType::Bool));
        assert_eq!(vtype("not true"), Some(ValueType::Bool));
    }

    #[test]
    fn a_variable_has_no_type_until_the_dialogue_runs() {
        assert_eq!(vtype("$gold"), Some(ValueType::Unknown));
        assert_eq!(vtype("$gold + 1"), Some(ValueType::Unknown));
        assert_eq!(vtype("-$gold"), Some(ValueType::Unknown));
        assert_eq!(vtype("max(1, 2)"), Some(ValueType::Unknown));
    }

    #[test]
    fn an_operator_that_always_gives_a_bool_stays_known() {
        assert_eq!(vtype("not $flag"), Some(ValueType::Bool));
        assert_eq!(vtype("$gold >= 50"), Some(ValueType::Bool));
        assert_eq!(vtype(r#"$name == "Alice""#), Some(ValueType::Bool));
        assert_eq!(vtype("$a and $b"), Some(ValueType::Bool));
    }

    #[test]
    fn error_on_a_unary_operator_that_does_not_take_its_operand() {
        assert_eq!(codes("not 1"), ["invalid-unary-operand"]);
        assert_eq!(codes(r#"-"a""#), ["invalid-unary-operand"]);
    }

    #[test]
    fn error_on_a_binary_operator_that_does_not_take_its_operands() {
        assert_eq!(codes(r#""a" - 1"#), ["invalid-binary-operands"]);
        assert_eq!(codes("1 and true"), ["invalid-binary-operands"]);
        assert_eq!(codes(r#"true == "a""#), ["invalid-binary-operands"]);
    }

    #[test]
    fn error_on_ordering_two_strings() {
        assert_eq!(codes(r#""a" < "b""#), ["invalid-binary-operands"]);
        assert_eq!(vtype(r#""a" == "b""#), Some(ValueType::Bool));
    }

    #[test]
    fn a_faulty_operand_is_reported_once() {
        assert_eq!(codes(r#"("a" - 1) + 1"#), ["invalid-binary-operands"]);
    }

    #[test]
    fn every_argument_of_a_call_is_checked() {
        assert_eq!(
            codes(r#"max("a" - 1, not 2)"#),
            ["invalid-binary-operands", "invalid-unary-operand"]
        );
    }

    #[test]
    fn the_diagnostic_points_at_the_operands_and_names_their_types() {
        let src = r#"$gold + ("a" - 1)"#;
        let mut diags = Diagnostics::from_src(src);
        let expr = ExprParser::new(&Spanned::from_text(src, 0), &mut diags).expr(0);

        assert!(!expr.value.is_type_valid(&mut diags));
        let diag = diags.iter().next().expect("one diagnostic");

        assert_eq!(&src[diag.span.start..diag.span.end], r#""a" - 1"#);
        let labels: Vec<_> = diag
            .labels
            .iter()
            .map(|l| (&src[l.span.start..l.span.end], l.message.as_str()))
            .collect();
        assert_eq!(labels, [(r#""a""#, "string"), ("1", "int")]);
    }
}
