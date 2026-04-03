use std::str::FromStr;

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
pub enum JsonNumber {
    I64(i64),
    U64(u64),
    F64(f64),
    Raw(String),
}

impl From<&serde_json::Number> for JsonNumber {
    fn from(value: &serde_json::Number) -> Self {
        if let Some(int) = value.as_i64() {
            Self::I64(int)
        } else if let Some(int) = value.as_u64() {
            Self::U64(int)
        } else if let Some(float) = value.as_f64() {
            Self::F64(float)
        } else {
            Self::Raw(value.to_string())
        }
    }
}

impl From<JsonNumber> for serde_json::Number {
    fn from(value: JsonNumber) -> Self {
        match value {
            JsonNumber::I64(int) => serde_json::Number::from(int),
            JsonNumber::U64(int) => serde_json::Number::from(int),
            JsonNumber::F64(float) => serde_json::Number::from_f64(float).unwrap_or_else(|| serde_json::Number::from(0)),
            JsonNumber::Raw(raw) => serde_json::Number::from_str(&raw).unwrap_or_else(|_| serde_json::Number::from(0)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct JsonObjectEntry {
    pub key: String,
    pub value: u32,
}

#[derive(Debug, Clone, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
pub enum JsonNodeKind {
    Null,
    Bool(bool),
    Number(JsonNumber),
    String(String),
    Array(Vec<u32>),
    Object(Vec<JsonObjectEntry>),
}

#[derive(Debug, Clone, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct JsonNode {
    pub kind: JsonNodeKind,
}

#[derive(Debug, Clone, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct JsonValue {
    pub root: u32,
    pub nodes: Vec<JsonNode>,
}

impl Default for JsonValue {
    fn default() -> Self {
        Self { root: 0, nodes: vec![JsonNode { kind: JsonNodeKind::Null }] }
    }
}

impl JsonValue {
    #[must_use]
    pub fn from_serde(value: &serde_json::Value) -> Self {
        let mut nodes = Vec::new();
        let root = push_node(value, &mut nodes);
        Self { root, nodes }
    }

    #[must_use]
    pub fn to_serde(&self) -> serde_json::Value {
        node_to_serde(self.root, &self.nodes)
    }
}

impl From<serde_json::Value> for JsonValue {
    fn from(value: serde_json::Value) -> Self {
        Self::from_serde(&value)
    }
}

impl From<&serde_json::Value> for JsonValue {
    fn from(value: &serde_json::Value) -> Self {
        Self::from_serde(value)
    }
}

impl From<JsonValue> for serde_json::Value {
    fn from(value: JsonValue) -> Self {
        value.to_serde()
    }
}

impl Serialize for JsonValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_serde().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for JsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(Self::from(value))
    }
}

fn push_node(value: &serde_json::Value, nodes: &mut Vec<JsonNode>) -> u32 {
    let kind = match value {
        serde_json::Value::Null => JsonNodeKind::Null,
        serde_json::Value::Bool(flag) => JsonNodeKind::Bool(*flag),
        serde_json::Value::Number(number) => JsonNodeKind::Number(JsonNumber::from(number)),
        serde_json::Value::String(text) => JsonNodeKind::String(text.clone()),
        serde_json::Value::Array(values) => JsonNodeKind::Array(values.iter().map(|entry| push_node(entry, nodes)).collect()),
        serde_json::Value::Object(values) => JsonNodeKind::Object(values.iter().map(|(key, value)| JsonObjectEntry { key: key.clone(), value: push_node(value, nodes) }).collect()),
    };

    let index = nodes.len() as u32;
    nodes.push(JsonNode { kind });
    index
}

fn node_to_serde(index: u32, nodes: &[JsonNode]) -> serde_json::Value {
    match &nodes[index as usize].kind {
        JsonNodeKind::Null => serde_json::Value::Null,
        JsonNodeKind::Bool(flag) => serde_json::Value::Bool(*flag),
        JsonNodeKind::Number(number) => serde_json::Value::Number(number.clone().into()),
        JsonNodeKind::String(text) => serde_json::Value::String(text.clone()),
        JsonNodeKind::Array(entries) => serde_json::Value::Array(entries.iter().map(|entry| node_to_serde(*entry, nodes)).collect()),
        JsonNodeKind::Object(entries) => serde_json::Value::Object(entries.iter().map(|entry| (entry.key.clone(), node_to_serde(entry.value, nodes))).collect()),
    }
}
