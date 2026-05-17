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
}
