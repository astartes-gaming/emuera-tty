use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i64),
    Str(String),
}

impl Value {
    pub fn as_int(&self) -> i64 {
        match self {
            Value::Int(i) => *i,
            Value::Str(s) => s.trim().parse().unwrap_or(0),
        }
    }
    pub fn into_string(self) -> String {
        match self {
            Value::Int(i) => i.to_string(),
            Value::Str(s) => s,
        }
    }
    pub fn as_string(&self) -> String {
        match self {
            Value::Int(i) => i.to_string(),
            Value::Str(s) => s.clone(),
        }
    }
    pub fn is_str(&self) -> bool {
        matches!(self, Value::Str(_))
    }
    pub fn truthy(&self) -> bool {
        match self {
            Value::Int(i) => *i != 0,
            Value::Str(s) => !s.is_empty(),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(i) => write!(f, "{}", i),
            Value::Str(s) => write!(f, "{}", s),
        }
    }
}

/// Non-local control flow and errors, propagated with `?`.
#[derive(Debug, Clone)]
pub enum Ctrl {
    Begin(String),
    Quit,
    /// runtime error: message, already-located flag
    Error(String, bool),
}

pub type VmResult<T> = Result<T, Ctrl>;

pub fn err<T>(msg: impl Into<String>) -> VmResult<T> {
    Err(Ctrl::Error(msg.into(), false))
}
