//! Script values, the VM's error/control type, and the MAP_* map type.

use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i64),
    Str(String),
}

impl Value {
    /// Integer view; strings parse as a number or count as 0.
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
    /// Condition value: non-zero, or a non-empty string.
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
    /// BEGIN <state>: unwind to the system loop and switch state
    Begin(String),
    /// QUIT
    Quit,
    /// runtime error: message, already-located flag
    Error(String, bool),
}

pub type VmResult<T> = Result<T, Ctrl>;

/// Shorthand for a not-yet-located runtime error.
pub fn err<T>(msg: impl Into<String>) -> VmResult<T> {
    Err(Ctrl::Error(msg.into(), false))
}

/// String→string map for Emuera-EM's MAP_* functions. Keys keep insertion
/// order, which MAP_GETKEYS exposes (C# Dictionary enumerates that way).
#[derive(Clone, Debug, Default)]
pub struct StrMap {
    keys: Vec<String>,
    vals: std::collections::HashMap<String, String>,
}

impl StrMap {
    pub fn get(&self, k: &str) -> Option<&String> {
        self.vals.get(k)
    }
    pub fn has(&self, k: &str) -> bool {
        self.vals.contains_key(k)
    }
    pub fn set(&mut self, k: String, v: String) {
        if !self.vals.contains_key(&k) {
            self.keys.push(k.clone());
        }
        self.vals.insert(k, v);
    }
    pub fn remove(&mut self, k: &str) {
        if self.vals.remove(k).is_some() {
            self.keys.retain(|x| x != k);
        }
    }
    pub fn clear(&mut self) {
        self.keys.clear();
        self.vals.clear();
    }
    pub fn len(&self) -> usize {
        self.keys.len()
    }
    pub fn keys(&self) -> &[String] {
        &self.keys
    }
    pub fn pairs(&self) -> impl Iterator<Item = (&String, &String)> {
        self.keys.iter().map(|k| (k, &self.vals[k]))
    }
}
