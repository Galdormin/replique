use std::{cmp::Ordering, collections::HashMap};

use thiserror::Error;

use crate::{
    dialogue::{Value, ValueType, builder::BuildError},
    host::{HostError, RepliqueHost},
    parser::{
        self,
        expr::{BinaryOp, UnaryOp},
    },
    vm::EvalCtx,
};

#[derive(Debug, Clone)]
pub(crate) enum Expr {
    Var(String),
    Litteral(Value),
    /// To represent `{name: Alan, hp: $base + 2}` we need HashMap of [`Expr`]
    LitteralDict(HashMap<String, Expr>),
    Attr {
        base: Box<Expr>,
        attrs: Vec<String>,
    },
    #[allow(unused)]
    Function {
        name: String,
        args: Vec<Expr>,
    },
    Unary {
        op: UnaryOp,
        rhs: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
}

impl TryFrom<parser::expr::Expr> for Expr {
    type Error = BuildError;

    fn try_from(value: parser::expr::Expr) -> Result<Self, BuildError> {
        match value {
            parser::expr::Expr::Var { name } => Ok(Expr::Var(name)),
            parser::expr::Expr::Function { name, args } => Ok(Expr::Function {
                name: name.value,
                args: args
                    .into_iter()
                    .map(|a| a.value.try_into())
                    .collect::<Result<_, _>>()?,
            }),
            parser::expr::Expr::Attr { base, attrs } => Ok(Expr::Attr {
                base: Box::new(base.value.try_into()?),
                attrs: attrs.into_iter().map(|s| s.value).collect(),
            }),
            parser::expr::Expr::Litteral { value } => Ok(Expr::Litteral(value.into())),
            parser::expr::Expr::LitteralDict(value) => {
                let map: HashMap<String, Expr> = value
                    .into_iter()
                    .map(|(key, val)| val.value.try_into().map(|expr| (key.value, expr)))
                    .collect::<Result<_, _>>()?;
                Ok(Expr::LitteralDict(map))
            }
            parser::expr::Expr::Unary { op, rhs } => Ok(Expr::Unary {
                op: op.value,
                rhs: Box::new(rhs.value.try_into()?),
            }),
            parser::expr::Expr::Binary { op, lhs, rhs } => Ok(Expr::Binary {
                op: op.value,
                lhs: Box::new(lhs.value.try_into()?),
                rhs: Box::new(rhs.value.try_into()?),
            }),
            parser::expr::Expr::Error => Err(BuildError::ErrorInExpr),
        }
    }
}

/// Why the `eval` has failed
#[derive(Error, Debug)]
pub enum EvalError {
    #[error("Uknown variable {0}")]
    UnknownVariable(String),
    /// A call in an expression: the host has no way to register a function
    /// yet, so none of them can be answered.
    #[error("Uknown function {0}")]
    UnknownFunction(String),
    /// The condition of an `[if]` evaluated to something else than a `bool`.
    /// Only a condition the parser could not type can get here.
    #[error("A condition must be a bool, received {0}")]
    NotACondition(String),
    #[error("Type mismatch for operator {op}: expected {expected} and received {received}")]
    TypeMismatch {
        op: String,
        expected: String,
        received: String,
    },
    #[error("Division by zero")]
    DivisionByZero,
    #[error("Integer overflow for operator {op}")]
    Overflow { op: String },
    #[error("Dict has no attribute {0}")]
    DictHasNoAttr(String),
    #[error("Attribute {name} expected Dict and received {received}")]
    AttrExpectedDict { name: String, received: String },
    #[error("Host error: {0}")]
    HostError(#[from] HostError),
}

impl Expr {
    /// Value of the expression, reading the variables it names from `vars`.
    pub(crate) fn eval<H: RepliqueHost>(
        &self,
        ctx: &mut EvalCtx<'_, H>,
    ) -> Result<Value, EvalError> {
        match self {
            Expr::Var(name) => ctx
                .vars
                .get(name)
                .cloned()
                .ok_or_else(|| EvalError::UnknownVariable(name.clone())),
            Expr::Attr { base, attrs } => {
                let mut val = base.eval(ctx)?;
                for attr in attrs {
                    // `eval` gives an owned value, so each step takes its
                    // entry out of the map instead of cloning the subtree.
                    val = match val {
                        Value::Dict(mut map) => map
                            .remove(attr)
                            .ok_or_else(|| EvalError::DictHasNoAttr(attr.clone()))?,
                        other => {
                            return Err(EvalError::AttrExpectedDict {
                                name: attr.clone(),
                                received: other.vtype().to_string(),
                            });
                        }
                    };
                }
                Ok(val)
            }
            Expr::Litteral(value) => Ok(value.clone()),
            Expr::LitteralDict(map) => {
                let map = map
                    .iter()
                    .map(|(key, expr)| expr.eval(ctx).map(|val| (key.clone(), val)))
                    .collect::<Result<_, _>>()?;
                Ok(Value::Dict(map))
            }
            Expr::Function { name, args } => {
                let args = args
                    .iter()
                    .map(|a| a.eval(ctx))
                    .collect::<Result<Vec<_>, _>>()?;

                ctx.call(name, args).map_err(EvalError::HostError)
            }
            Expr::Unary { op, rhs } => unary(*op, rhs.eval(ctx)?),
            Expr::Binary { op, lhs, rhs } => binary(*op, lhs.eval(ctx)?, rhs.eval(ctx)?),
        }
    }
}

fn unary(op: UnaryOp, rhs: Value) -> Result<Value, EvalError> {
    match op {
        UnaryOp::Not => match rhs {
            Value::Bool(val) => Ok(Value::Bool(!val)),
            val => Err(EvalError::TypeMismatch {
                op: op.to_string(),
                expected: ValueType::Bool.to_string(),
                received: val.vtype().to_string(),
            }),
        },
        UnaryOp::Neg => match rhs {
            Value::Int(val) => val
                .checked_neg()
                .map(Value::Int)
                .ok_or(EvalError::Overflow { op: op.to_string() }),
            Value::Float(val) => Ok(Value::Float(-val)),
            val => Err(EvalError::TypeMismatch {
                op: op.to_string(),
                expected: format!("{} or {}", ValueType::Int, ValueType::Float),
                received: val.vtype().to_string(),
            }),
        },
    }
}

fn binary(op: BinaryOp, lhs: Value, rhs: Value) -> Result<Value, EvalError> {
    match op {
        BinaryOp::Or => Ok(Value::Bool(as_bool(op, lhs)? || as_bool(op, rhs)?)),
        BinaryOp::And => Ok(Value::Bool(as_bool(op, lhs)? && as_bool(op, rhs)?)),

        BinaryOp::Eq => Ok(Value::Bool(equals(op, lhs, rhs)?)),
        BinaryOp::Ne => Ok(Value::Bool(!equals(op, lhs, rhs)?)),

        BinaryOp::Lt => Ok(Value::Bool(matches!(
            compare(op, lhs, rhs)?,
            Ordering::Less
        ))),
        BinaryOp::Le => Ok(Value::Bool(matches!(
            compare(op, lhs, rhs)?,
            Ordering::Less | Ordering::Equal
        ))),
        BinaryOp::Gt => Ok(Value::Bool(matches!(
            compare(op, lhs, rhs)?,
            Ordering::Greater
        ))),
        BinaryOp::Ge => Ok(Value::Bool(matches!(
            compare(op, lhs, rhs)?,
            Ordering::Greater | Ordering::Equal
        ))),

        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => arith(op, lhs, rhs),
    }
}

/// Both sides of an operator as numbers of the same kind.
enum Nums {
    Ints(i64, i64),
    Floats(f64, f64),
}

impl Nums {
    /// Both sides as floats, whether or not they were written as such. Used by
    /// `/`, which never gives an `Int` back.
    fn floats(self) -> (f64, f64) {
        match self {
            Self::Ints(lhs, rhs) => (lhs as f64, rhs as f64),
            Self::Floats(lhs, rhs) => (lhs, rhs),
        }
    }
}

/// Reads both sides as numbers, an `Int` facing a `Float` being cast, or
/// names the side that is not a number.
fn nums(op: BinaryOp, lhs: Value, rhs: Value) -> Result<Nums, EvalError> {
    match (lhs, rhs) {
        (Value::Int(lhs), Value::Int(rhs)) => Ok(Nums::Ints(lhs, rhs)),
        (Value::Float(lhs), Value::Float(rhs)) => Ok(Nums::Floats(lhs, rhs)),
        (Value::Int(lhs), Value::Float(rhs)) => Ok(Nums::Floats(lhs as f64, rhs)),
        (Value::Float(lhs), Value::Int(rhs)) => Ok(Nums::Floats(lhs, rhs as f64)),
        (lhs, rhs) => {
            let culprit = match lhs {
                Value::Int(_) | Value::Float(_) => rhs,
                lhs => lhs,
            };
            Err(EvalError::TypeMismatch {
                op: op.to_string(),
                expected: format!("{} or {}", ValueType::Int, ValueType::Float),
                received: culprit.vtype().to_string(),
            })
        }
    }
}

/// `+`, `-`, `*` and `/` on two numbers.
///
/// Two `Int` give an `Int`, except for `/`
fn arith(op: BinaryOp, lhs: Value, rhs: Value) -> Result<Value, EvalError> {
    let nums = nums(op, lhs, rhs)?;

    if matches!(op, BinaryOp::Div) {
        let (lhs, rhs) = nums.floats();
        if rhs == 0.0 {
            return Err(EvalError::DivisionByZero);
        }
        return Ok(Value::Float(lhs / rhs));
    }

    match nums {
        Nums::Ints(lhs, rhs) => match op {
            BinaryOp::Add => lhs.checked_add(rhs),
            BinaryOp::Sub => lhs.checked_sub(rhs),
            BinaryOp::Mul => lhs.checked_mul(rhs),
            _ => unreachable!("only `+`, `-`, `*` and `/` reach here"),
        }
        .map(Value::Int)
        .ok_or(EvalError::Overflow { op: op.to_string() }),
        Nums::Floats(lhs, rhs) => Ok(Value::Float(match op {
            BinaryOp::Add => lhs + rhs,
            BinaryOp::Sub => lhs - rhs,
            BinaryOp::Mul => lhs * rhs,
            _ => unreachable!("only `+`, `-`, `*` and `/` reach here"),
        })),
    }
}

/// Two values of the same type, `Int` and `Float` counting as one. Comparing
/// types that have nothing to do with each other is a mistake, not a `false`.
fn equals(op: BinaryOp, lhs: Value, rhs: Value) -> Result<bool, EvalError> {
    match (lhs, rhs) {
        (Value::Bool(lhs), Value::Bool(rhs)) => Ok(lhs == rhs),
        (Value::String(lhs), Value::String(rhs)) => Ok(lhs == rhs),
        (Value::Dict(lhs), Value::Dict(rhs)) => Ok(lhs == rhs),
        (lhs, rhs) if lhs.vtype().is_number() && rhs.vtype().is_number() => {
            Ok(match nums(op, lhs, rhs)? {
                Nums::Ints(lhs, rhs) => lhs == rhs,
                Nums::Floats(lhs, rhs) => lhs == rhs,
            })
        }
        (lhs, rhs) => Err(EvalError::TypeMismatch {
            op: op.to_string(),
            expected: lhs.vtype().to_string(),
            received: rhs.vtype().to_string(),
        }),
    }
}

/// Order of two numbers, or `None` when they cannot be ordered.
/// This ignore NaN or Inf
fn compare(op: BinaryOp, lhs: Value, rhs: Value) -> Result<Ordering, EvalError> {
    Ok(match nums(op, lhs, rhs)? {
        Nums::Ints(lhs, rhs) => lhs.cmp(&rhs),
        Nums::Floats(lhs, rhs) => lhs.total_cmp(&rhs),
    })
}

/// Cast as bool or [`EvalError::TypeMismatch`]
fn as_bool(op: BinaryOp, value: Value) -> Result<bool, EvalError> {
    match value {
        Value::Bool(val) => Ok(val),
        val => Err(EvalError::TypeMismatch {
            op: op.to_string(),
            expected: ValueType::Bool.to_string(),
            received: val.vtype().to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        host::{NoHost, test::TestHost},
        vm::VarStore,
    };

    use super::*;

    fn bin(op: BinaryOp, lhs: Value, rhs: Value) -> Result<Value, EvalError> {
        binary(op, lhs, rhs)
    }

    /// Value of `expr` with no variable, against the host it is given.
    fn eval_with<H: RepliqueHost>(expr: &Expr, host: &mut H) -> Result<Value, EvalError> {
        expr.eval(&mut EvalCtx {
            vars: &VarStore::default(),
            host,
        })
    }

    fn call(name: &str, args: Vec<Expr>) -> Expr {
        Expr::Function {
            name: name.to_owned(),
            args,
        }
    }

    fn int(value: i64) -> Expr {
        Expr::Litteral(Value::Int(value))
    }

    fn str(text: &str) -> Value {
        Value::String(text.to_owned())
    }

    fn dict(entries: &[(&str, Value)]) -> Value {
        Value::Dict(
            entries
                .iter()
                .map(|(key, val)| ((*key).to_owned(), val.clone()))
                .collect(),
        )
    }

    #[test]
    fn a_function_is_answered_by_the_host() {
        let mut host = TestHost::default();

        let value = eval_with(
            &call("upper_new", vec![Expr::Litteral(str("alice"))]),
            &mut host,
        );

        assert_eq!(value.unwrap(), str("ALICE"));
        assert_eq!(host.calls, [("upper_new".to_owned(), vec![str("alice")])]);
    }

    /// The host is handed values, never expressions: what an argument is made
    /// of is settled before the call.
    #[test]
    fn the_arguments_of_a_function_reach_the_host_evaluated() {
        let mut host = TestHost::default();
        let arg = Expr::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(int(1)),
            rhs: Box::new(int(2)),
        };

        let value = eval_with(&call("double", vec![arg]), &mut host);

        assert_eq!(value.unwrap(), Value::Int(6));
        assert_eq!(host.calls, [("double".to_owned(), vec![Value::Int(3)])]);
    }

    #[test]
    fn a_function_is_a_value_like_any_other() {
        let mut host = TestHost::default();
        let expr = Expr::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(call("double", vec![call("double", vec![int(2)])])),
            rhs: Box::new(int(1)),
        };

        assert_eq!(eval_with(&expr, &mut host).unwrap(), Value::Int(9));
        // The inner call runs first, and its result is what the outer one gets.
        assert_eq!(
            host.calls,
            [
                ("double".to_owned(), vec![Value::Int(2)]),
                ("double".to_owned(), vec![Value::Int(4)]),
            ]
        );
    }

    #[test]
    fn a_variable_reaches_the_host_as_its_value() {
        let vars = VarStore::new(HashMap::from([("name".to_owned(), str("bob"))]));
        let mut host = TestHost::default();

        let value = call("upper", vec![Expr::Var("name".to_owned())]).eval(&mut EvalCtx {
            vars: &vars,
            host: &mut host,
        });

        assert_eq!(value.unwrap(), str("BOB"));
    }

    #[test]
    fn error_on_a_function_the_host_does_not_know() {
        let mut host = TestHost::default();

        let err = eval_with(&call("unknown", vec![]), &mut host).unwrap_err();

        assert!(matches!(
            err,
            EvalError::HostError(HostError::UnknownFunction(name)) if name == "unknown"
        ));
    }

    /// A host that refuses the call is not a panic and not a `None`: the
    /// expression fails, and the VM turns that into an error of its own.
    #[test]
    fn error_on_a_function_the_host_refuses_to_answer() {
        let mut host = TestHost::default();

        let err = eval_with(&call("boom", vec![]), &mut host).unwrap_err();

        assert!(matches!(
            err,
            EvalError::HostError(HostError::Failed { ref name, .. }) if name == "boom"
        ));
    }

    #[test]
    fn no_host_answers_no_function_at_all() {
        let err = eval_with(
            &call("upper_new", vec![Expr::Litteral(str("A"))]),
            &mut NoHost,
        )
        .unwrap_err();

        assert!(matches!(
            err,
            EvalError::HostError(HostError::UnknownFunction(_))
        ));
    }

    /// An argument that does not evaluate stops the call before it is made,
    /// so the host never sees a half-built one.
    #[test]
    fn an_argument_that_does_not_evaluate_never_reaches_the_host() {
        let mut host = TestHost::default();

        let err = eval_with(
            &call("double", vec![Expr::Var("missing".to_owned())]),
            &mut host,
        )
        .unwrap_err();

        assert!(matches!(err, EvalError::UnknownVariable(_)));
        assert_eq!(host.called(), [] as [&str; 0]);
    }

    #[test]
    fn two_dicts_are_equal_when_they_hold_the_same_entries() {
        let left = dict(&[("hp", Value::Int(1)), ("name", str("Leon"))]);
        let right = dict(&[("name", str("Leon")), ("hp", Value::Int(1))]);

        assert_eq!(
            bin(BinaryOp::Eq, left.clone(), right).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            bin(BinaryOp::Eq, left.clone(), dict(&[("hp", Value::Int(2))])).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            bin(BinaryOp::Ne, left.clone(), dict(&[])).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn a_dict_compared_to_anything_else_is_a_type_mismatch() {
        let left = dict(&[("hp", Value::Int(1))]);

        assert!(matches!(
            bin(BinaryOp::Eq, left.clone(), Value::Int(1)),
            Err(EvalError::TypeMismatch { .. })
        ));
        assert!(matches!(
            bin(BinaryOp::Add, left, Value::Int(1)),
            Err(EvalError::TypeMismatch { .. })
        ));
    }

    #[test]
    fn an_attr_reads_its_way_down_the_dicts() {
        let vars = HashMap::from([(
            "player".to_owned(),
            dict(&[("stats", dict(&[("hp", Value::Int(12))]))]),
        )]);
        let mut ctx = EvalCtx {
            vars: &VarStore::new(vars),
            host: &mut NoHost,
        };
        let attr = |attrs: &[&str]| Expr::Attr {
            base: Box::new(Expr::Var("player".to_owned())),
            attrs: attrs.iter().map(|a| (*a).to_owned()).collect(),
        };

        assert_eq!(
            attr(&["stats", "hp"]).eval(&mut ctx).unwrap(),
            Value::Int(12)
        );
        assert!(matches!(
            attr(&["stats", "mp"]).eval(&mut ctx),
            Err(EvalError::DictHasNoAttr(_))
        ));
        assert!(matches!(
            attr(&["stats", "hp", "deeper"]).eval(&mut ctx),
            Err(EvalError::AttrExpectedDict { .. })
        ));
    }

    #[test]
    fn and_and_or_take_booleans_only() {
        use BinaryOp::{And, Or};

        assert_eq!(
            bin(And, Value::Bool(true), Value::Bool(false)).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            bin(Or, Value::Bool(true), Value::Bool(false)).unwrap(),
            Value::Bool(true)
        );
        assert!(matches!(
            bin(And, Value::Int(1), Value::Bool(true)),
            Err(EvalError::TypeMismatch { .. })
        ));
    }

    #[test]
    fn an_int_meeting_a_float_is_promoted() {
        assert_eq!(
            bin(BinaryOp::Add, Value::Int(1), Value::Float(0.5)).unwrap(),
            Value::Float(1.5)
        );
        assert_eq!(
            bin(BinaryOp::Eq, Value::Int(1), Value::Float(1.0)).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            bin(BinaryOp::Lt, Value::Int(1), Value::Float(1.5)).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn arithmetic_on_two_ints_stays_an_int() {
        assert_eq!(
            bin(BinaryOp::Add, Value::Int(2), Value::Int(3)).unwrap(),
            Value::Int(5)
        );
        assert_eq!(
            bin(BinaryOp::Sub, Value::Int(2), Value::Int(3)).unwrap(),
            Value::Int(-1)
        );
        assert_eq!(
            bin(BinaryOp::Mul, Value::Int(2), Value::Int(3)).unwrap(),
            Value::Int(6)
        );
    }

    #[test]
    fn a_division_always_gives_a_float() {
        assert_eq!(
            bin(BinaryOp::Div, Value::Int(7), Value::Int(2)).unwrap(),
            Value::Float(3.5)
        );
    }

    #[test]
    fn a_string_is_never_turned_into_a_number() {
        let err = bin(BinaryOp::Add, str("2"), Value::Int(2)).unwrap_err();

        assert!(matches!(
            err,
            EvalError::TypeMismatch { ref received, .. } if received == "string"
        ));
    }

    #[test]
    fn two_types_that_have_nothing_in_common_do_not_compare() {
        assert!(matches!(
            bin(BinaryOp::Eq, Value::Bool(true), str("true")),
            Err(EvalError::TypeMismatch { .. })
        ));
        assert!(matches!(
            bin(BinaryOp::Lt, str("a"), str("b")),
            Err(EvalError::TypeMismatch { .. })
        ));
    }

    #[test]
    fn values_of_the_same_type_compare_as_expected() {
        assert_eq!(
            bin(BinaryOp::Eq, str("a"), str("a")).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            bin(BinaryOp::Ne, Value::Bool(true), Value::Bool(false)).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            bin(BinaryOp::Ge, Value::Int(2), Value::Int(2)).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn error_on_a_division_by_zero() {
        assert!(matches!(
            bin(BinaryOp::Div, Value::Int(1), Value::Int(0)),
            Err(EvalError::DivisionByZero)
        ));
        assert!(matches!(
            bin(BinaryOp::Div, Value::Float(1.0), Value::Float(0.0)),
            Err(EvalError::DivisionByZero)
        ));
    }

    /// An overflow is an error and not a wrap around, since the VM promises
    /// never to panic and a debug build would.
    #[test]
    fn error_on_an_integer_overflow() {
        assert!(matches!(
            bin(BinaryOp::Add, Value::Int(i64::MAX), Value::Int(1)),
            Err(EvalError::Overflow { .. })
        ));
        assert!(matches!(
            unary(UnaryOp::Neg, Value::Int(i64::MIN)),
            Err(EvalError::Overflow { .. })
        ));
    }

    #[test]
    fn a_unary_operator_takes_what_it_can_use() {
        assert_eq!(
            unary(UnaryOp::Not, Value::Bool(true)).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(unary(UnaryOp::Neg, Value::Int(2)).unwrap(), Value::Int(-2));
        assert_eq!(
            unary(UnaryOp::Neg, Value::Float(0.5)).unwrap(),
            Value::Float(-0.5)
        );
        assert!(matches!(
            unary(UnaryOp::Not, Value::Int(1)),
            Err(EvalError::TypeMismatch { .. })
        ));
    }
}
