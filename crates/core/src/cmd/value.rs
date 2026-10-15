use std::sync::Arc;

use compact_str::CompactString;

use crate::loc::Location;

/// Значение аргумента команды. Набор вариантов закрыт (S0 v5 №4: `Text` не вводится).
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Str(CompactString),
    /// В текст и TOML пишется как uri-строка и читается обратно как `Str`;
    /// [`Args::loc`](super::Args::loc) принимает оба варианта.
    Loc(Location),
    List(Arc<[Value]>),
}

/// Тип значения для [`ArgSpec`](super::ArgSpec).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ValueKind {
    Bool,
    Int,
    Str,
    /// Принимает `Loc` и `Str` (строка разбирается в `Args::loc`).
    Loc,
    List,
}

impl Value {
    pub fn kind(&self) -> ValueKind {
        match self {
            Value::Bool(_) => ValueKind::Bool,
            Value::Int(_) => ValueKind::Int,
            Value::Str(_) => ValueKind::Str,
            Value::Loc(_) => ValueKind::Loc,
            Value::List(_) => ValueKind::List,
        }
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Int(v.into())
    }
}

impl From<u32> for Value {
    fn from(v: u32) -> Self {
        Value::Int(v.into())
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Str(v.into())
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Str(v.into())
    }
}

impl From<CompactString> for Value {
    fn from(v: CompactString) -> Self {
        Value::Str(v)
    }
}

impl From<Location> for Value {
    fn from(v: Location) -> Self {
        Value::Loc(v)
    }
}

impl From<Vec<Value>> for Value {
    fn from(v: Vec<Value>) -> Self {
        Value::List(v.into())
    }
}
