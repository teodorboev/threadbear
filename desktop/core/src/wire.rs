//! Tolerant protocol fields. Read each value before trying its type so an
//! error cannot leave a streaming deserializer halfway through the next row.
//! Diagnostics contain Rust type names and positions only, never serde's
//! error text (which can contain the value it could not read).
use serde::{de::DeserializeOwned, Deserialize, Deserializer};
use serde_json::Value;
use std::{collections::{HashMap, HashSet}, sync::{Mutex, OnceLock}};

static LOGGER: OnceLock<fn(&str)> = OnceLock::new();
// Accessors also decode raw nested fields while drawing. Report a bad field
// once per source location and type, so version skew cannot write every frame
// or fill a log with one line per bad row. Keys are all compile-time metadata.
static REPORTED: Mutex<Option<HashSet<(&'static str, u32, u32, &'static str)>>> = Mutex::new(None);

pub fn set_logger(logger: fn(&str)) {
    let _ = LOGGER.set(logger);
}

#[track_caller]
pub fn dropped<T>(kind: &str) {
    let at = std::panic::Location::caller();
    let ty = std::any::type_name::<T>();
    if let Ok(mut reported) = REPORTED.lock() {
        if !reported.get_or_insert_with(HashSet::new).insert((at.file(), at.line(), at.column(), ty)) { return; }
    }
    let message = format!("dropped invalid {kind} ({ty}) at {}:{}:{}; repeats at this field are suppressed", at.file(), at.line(), at.column());
    if let Some(log) = LOGGER.get() { log(&message); } else { eprintln!("threadbear: decode {message}"); }
}

#[track_caller]
pub fn read<T: DeserializeOwned>(value: Value) -> Option<T> {
    if value.is_null() { return None; }
    match serde_json::from_value(value) {
        Ok(value) => Some(value),
        Err(_) => { dropped::<T>("row"); None }
    }
}

#[track_caller]
pub fn field<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(d: D) -> Result<T, D::Error> {
    let value = Value::deserialize(d)?;
    Ok(match serde_json::from_value(value) {
        Ok(value) => value,
        Err(_) => { dropped::<T>("field"); T::default() }
    })
}

pub fn id<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let value = Value::deserialize(d)?;
    value.as_str().filter(|id| !id.is_empty()).map(str::to_owned).ok_or_else(|| serde::de::Error::custom("invalid row identity"))
}

#[track_caller]
pub fn list<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Vec<T>, D::Error> {
    let value = Value::deserialize(d)?;
    Ok(rows(value))
}

#[track_caller]
fn rows<T: DeserializeOwned>(value: Value) -> Vec<T> {
    match value {
        Value::Array(rows) => {
            let mut kept = Vec::new();
            let mut first_bad = None;
            for (i, row) in rows.into_iter().enumerate() {
                match serde_json::from_value(row) {
                    Ok(row) => kept.push(row),
                    Err(_) => { first_bad.get_or_insert(i); }
                }
            }
            if let Some(i) = first_bad { dropped::<T>(&format!("list entry at {i}")); }
            kept
        }
        _ => { dropped::<Vec<T>>("list field"); vec![] }
    }
}

#[track_caller]
pub fn optional_list<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<Vec<T>>, D::Error> {
    let value = Value::deserialize(d)?;
    Ok(match value {
        Value::Null => None,
        Value::Array(_) => Some(rows(value)),
        _ => { dropped::<Vec<T>>("optional list field"); None }
    })
}

#[track_caller]
pub fn map<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<HashMap<String, T>, D::Error> {
    let value = Value::deserialize(d)?;
    Ok(match value {
        Value::Object(entries) => {
            let mut kept = HashMap::new();
            let mut bad = false;
            for (key, value) in entries {
                match serde_json::from_value(value) {
                    Ok(value) => { kept.insert(key, value); }
                    Err(_) => bad = true,
                }
            }
            // Keys can be user text too.
            if bad { dropped::<HashMap<String, T>>("map entry"); }
            kept
        }
        _ => { dropped::<HashMap<String, T>>("map field"); HashMap::new() }
    })
}

#[track_caller]
pub fn default_true<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    let value = Value::deserialize(d)?;
    Ok(value.as_bool().unwrap_or_else(|| { dropped::<bool>("default-on field"); true }))
}

/// Sequence and count fields also arrive as strings from older mirrors.
/// Keep them within JavaScript's exact integer range, as event seqs are.
fn uint_value(value: &Value) -> Option<u64> {
    const MOST: u64 = 9_007_199_254_740_991;
    let integer = value.as_u64().or_else(|| value.as_str().and_then(|s| s.parse().ok())).or_else(|| {
        value.as_f64().filter(|n| n.is_finite() && *n >= 0. && n.fract() == 0. && *n <= MOST as f64).map(|n| n as u64)
    })?;
    (integer <= MOST).then_some(integer)
}

#[track_caller]
pub fn uint<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let value = Value::deserialize(d)?;
    Ok(uint_value(&value).unwrap_or_else(|| { dropped::<u64>("integer field"); 0 }))
}

#[track_caller]
pub fn optional_uint<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    let value = Value::deserialize(d)?;
    if value.is_null() { return Ok(None); }
    let number = uint_value(&value);
    if number.is_none() { dropped::<Option<u64>>("integer field"); }
    Ok(number)
}
