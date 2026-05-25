// Stub types fleshed out as M1 lands. Dead-code allow keeps clippy pedantic
// quiet until the parser starts producing values.
#![allow(dead_code)]

use std::collections::BTreeMap;

/// A decoded KV3 value tree.
#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Double(f64),
    String(String),
    Binary(Vec<u8>),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

/// Borrowing view into a KV3 [`Value`]. Helpful for cheap lookups inside the
/// texture header without cloning.
#[derive(Debug, Clone, Copy)]
pub enum ValueRef<'a> {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Double(f64),
    String(&'a str),
    Binary(&'a [u8]),
    Array(&'a [Value]),
    Object(&'a BTreeMap<String, Value>),
}

impl Value {
    pub fn as_ref(&self) -> ValueRef<'_> {
        match self {
            Self::Null => ValueRef::Null,
            Self::Bool(b) => ValueRef::Bool(*b),
            Self::Int(i) => ValueRef::Int(*i),
            Self::UInt(u) => ValueRef::UInt(*u),
            Self::Double(d) => ValueRef::Double(*d),
            Self::String(s) => ValueRef::String(s.as_str()),
            Self::Binary(b) => ValueRef::Binary(b.as_slice()),
            Self::Array(a) => ValueRef::Array(a.as_slice()),
            Self::Object(o) => ValueRef::Object(o),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Object(o) => o.get(key),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            Self::UInt(u) => i64::try_from(*u).ok(),
            _ => None,
        }
    }

    pub fn as_uint(&self) -> Option<u64> {
        match self {
            Self::UInt(u) => Some(*u),
            Self::Int(i) => u64::try_from(*i).ok(),
            _ => None,
        }
    }

    /// Reads a numeric leaf as `f64`. KV3 stores `FLOAT`/`DOUBLE` as
    /// [`Value::Double`]; integers widen (lossy past 2^53, which model data
    /// never reaches for the numeric fields we read this way).
    #[allow(clippy::cast_precision_loss)]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Double(d) => Some(*d),
            Self::Int(i) => Some(*i as f64),
            Self::UInt(u) => Some(*u as f64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Self::Array(a) => Some(a.as_slice()),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Self::Object(o) => Some(o),
            _ => None,
        }
    }

    /// Convenience: a child value looked up by key, returned as `f64`.
    pub fn get_f64(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(Value::as_f64)
    }
}
