use serde::Serialize;
use url::Url;

pub mod registry;
pub use registry::SchemaRegistry;

pub mod types;
pub use types::{AsyncApiData, AsyncApiDataMap, AsyncApiPath, AsyncApiPayload, FromSegments};

#[derive(Debug, Clone)]
pub struct TypeSchema {
    pub name: &'static str,
    pub schema: serde_json::Value,
}

pub trait SchemaProvider {
    const NAME: &'static str;
    fn schema() -> serde_json::Value;
    fn example() -> Option<serde_json::Value> {
        None
    }
    fn register_schemas(map: &mut std::collections::BTreeMap<String, serde_json::Value>) {
        map.entry(Self::NAME.to_string()).or_insert_with(Self::schema);
    }
}

macro_rules! simple_schema {
    ($t:ty, $name:expr) => {
        impl SchemaProvider for $t {
            const NAME: &'static str = $name;
            fn schema() -> serde_json::Value {
                serde_json::to_value(&schemars::schema_for!($t)).unwrap()
            }
            fn example() -> Option<serde_json::Value> {
                None
            }
            fn register_schemas(map: &mut std::collections::BTreeMap<String, serde_json::Value>) {
                map.entry(Self::NAME.to_string()).or_insert_with(Self::schema);
            }
        }
    };
}

simple_schema!(u32, "U32");
simple_schema!(i32, "I32");
simple_schema!(f32, "F32");
simple_schema!(f64, "F64");
simple_schema!(bool, "Boolean");
simple_schema!(String, "String");
simple_schema!(serde_json::Value, "Value");
simple_schema!(u64, "U64");
simple_schema!(usize, "Usize");

impl<T: SchemaProvider> SchemaProvider for Option<T> {
    const NAME: &'static str = T::NAME;
    fn schema() -> serde_json::Value {
        let mut base = T::schema();
        if let serde_json::Value::Object(ref mut obj) = base {
            obj.insert("nullable".into(), serde_json::Value::Bool(true));
        }
        base
    }
    fn example() -> Option<serde_json::Value> {
        T::example()
    }
    fn register_schemas(map: &mut std::collections::BTreeMap<String, serde_json::Value>) {
        T::register_schemas(map);
    }
}

impl<T: SchemaProvider> SchemaProvider for Vec<T> {
    const NAME: &'static str = "array";
    fn schema() -> serde_json::Value {
        serde_json::json!({"type": "array", "items": T::schema()})
    }
    fn example() -> Option<serde_json::Value> {
        T::example().map(|e| serde_json::json!([e]))
    }
    fn register_schemas(map: &mut std::collections::BTreeMap<String, serde_json::Value>) {
        T::register_schemas(map);
    }
}

impl<T: SchemaProvider + Clone, const N: usize> SchemaProvider for [T; N] {
    const NAME: &'static str = "array";
    fn schema() -> serde_json::Value {
        serde_json::json!({
            "type": "array",
            "items": T::schema(),
            "minItems": N,
            "maxItems": N
        })
    }
    fn example() -> Option<serde_json::Value> {
        T::example().map(|e| {
            let arr: Vec<_> = std::iter::repeat_n(e, N).collect();
            serde_json::Value::Array(arr)
        })
    }
    fn register_schemas(map: &mut std::collections::BTreeMap<String, serde_json::Value>) {
        T::register_schemas(map);
    }
}

impl<T: SchemaProvider> SchemaProvider for (T,) {
    const NAME: &'static str = "tuple";
    fn schema() -> serde_json::Value {
        serde_json::json!({
            "type": "array",
            "items": T::schema(),
            "minItems": 1,
            "maxItems": 1
        })
    }
    fn example() -> Option<serde_json::Value> {
        T::example().map(|e| serde_json::json!([e]))
    }
    fn register_schemas(map: &mut std::collections::BTreeMap<String, serde_json::Value>) {
        T::register_schemas(map);
    }
}

#[derive(Debug, Clone)]
pub struct WsDoc {
    pub path: &'static str,
    pub summary: &'static str,
    pub description: &'static str,
    pub tags: Vec<String>,
    pub payload: Option<TypeSchema>,
    pub responses: Vec<TypeSchema>,
    pub params: Vec<(String, TypeSchema)>,
}

pub type WsDocList = Vec<WsDoc>;

pub trait DocumentedCommand {
    fn doc() -> Option<WsDoc> {
        None
    }
    fn register_schemas(_map: &mut std::collections::BTreeMap<String, serde_json::Value>) {}
}

pub trait DocumentedLoop {
    fn doc() -> Option<WsDoc> {
        None
    }
    fn register_schemas(_map: &mut std::collections::BTreeMap<String, serde_json::Value>) {}
}

#[derive(Debug, Clone, Serialize)]
pub struct AsyncApiInfo {
    pub title: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "termsOfService", skip_serializing_if = "Option::is_none")]
    pub terms_of_service: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact: Option<Contact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<License>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
    #[serde(rename = "externalDocs", skip_serializing_if = "Option::is_none")]
    pub external_docs: Option<ExternalDocs>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Contact {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct License {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExternalDocs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub url: String,
}

impl Default for AsyncApiInfo {
    fn default() -> Self {
        Self { title: "WebSocket API".into(), version: "1.0.0".into(), summary: None, description: None, terms_of_service: None, contact: None, license: None, tags: Vec::new(), external_docs: None }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RefObj {
    #[serde(rename = "$ref")]
    pub r#ref: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Tag {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "externalDocs", skip_serializing_if = "Option::is_none")]
    pub external_docs: Option<ExternalDocs>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Server {
    pub host: String,
    pub protocol: String,
    #[serde(rename = "protocolVersion", skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Parameter {
    #[serde(rename = "x-schema")]
    pub schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub summary: &'static str,
    pub description: &'static str,
    pub tags: Vec<Tag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Channel {
    pub address: String,
    pub messages: std::collections::BTreeMap<String, Message>,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub parameters: std::collections::BTreeMap<String, Parameter>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Operation {
    pub action: &'static str,
    pub channel: RefObj,
    pub messages: Vec<RefObj>,
    pub summary: &'static str,
    pub description: &'static str,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Components {
    pub schemas: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AsyncApiDoc {
    pub asyncapi: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Url>,
    pub info: AsyncApiInfo,
    #[serde(rename = "defaultContentType")]
    pub default_content_type: &'static str,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub servers: std::collections::BTreeMap<String, Server>,
    pub channels: std::collections::BTreeMap<String, Channel>,
    pub operations: std::collections::BTreeMap<String, Operation>,
    pub components: Components,
}
use std::collections::BTreeMap;

fn escape_json_pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

#[allow(clippy::too_many_arguments)]
pub fn generate_asyncapi(
    docs: &[WsDoc],
    loop_docs: &[WsDoc],
    schemas: &SchemaRegistry,
    mut info: AsyncApiInfo,
    servers: BTreeMap<String, Server>,
    id: Option<Url>,
    tags: &[Tag],
    external: Option<ExternalDocs>,
) -> AsyncApiDoc {
    let mut channels: BTreeMap<String, Channel> = BTreeMap::new();
    let mut operations: BTreeMap<String, Operation> = BTreeMap::new();
    let mut schema_map = schemas.collect();

    const PRIMITIVES: &[&str] = &["U32", "U64", "Usize", "I32", "F32", "F64", "Boolean", "String"];
    let is_primitive = |name: &str| PRIMITIVES.contains(&name);

    let wrap_response = |schema: serde_json::Value, cmd: &str| {
        serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "cmd": {"type": "string", "example": cmd},
                "payload": schema,
                "sent-at": {"type": "string", "format": "date-time"}
            },
            "required": ["cmd", "payload", "sent-at"]
        })
    };

    for doc in docs {
        let msg_id: String = doc.path.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();

        let payload_val = if let Some(ts) = &doc.payload {
            let schema = if is_primitive(ts.name) {
                ts.schema.clone()
            } else {
                schema_map.entry(ts.name.to_string()).or_insert_with(|| ts.schema.clone());
                serde_json::json!({"$ref": format!("#/components/schemas/{}", ts.name)})
            };
            Some(serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "cmd": {"type": "string", "example": doc.path},
                    "payload": schema,
                    "sent-at": {"type": "string", "format": "date-time"}
                },
                "required": ["cmd", "payload", "sent-at"]
            }))
        } else {
            Some(serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "cmd": {"type": "string", "example": doc.path},
                    "sent-at": {"type": "string", "format": "date-time"}
                },
                "required": ["cmd", "sent-at"]
            }))
        };

        let mut response_vals = Vec::new();
        for ts in &doc.responses {
            if is_primitive(ts.name) {
                response_vals.push(wrap_response(ts.schema.clone(), doc.path));
            } else {
                schema_map.entry(ts.name.to_string()).or_insert_with(|| ts.schema.clone());
                response_vals.push(wrap_response(serde_json::json!({"$ref": format!("#/components/schemas/{}", ts.name)}), doc.path));
            }
        }

        let base_tags: Vec<Tag> = doc.tags.iter().cloned().map(|t| Tag { name: t, description: None, external_docs: None }).collect();

        let request_message = Message { summary: doc.summary, description: doc.description, tags: base_tags.clone(), payload: payload_val.clone() };

        let mut messages_map: BTreeMap<String, Message> = BTreeMap::new();
        messages_map.insert(msg_id.clone(), request_message);

        let channel_pointer = format!("#/channels/{}", escape_json_pointer_token(doc.path));

        if !doc.responses.is_empty() {
            let mut resp_refs = Vec::new();
            for (idx, val) in response_vals.iter().enumerate() {
                let resp_id = format!("{msg_id}_response_{idx}");
                let resp_message = Message { summary: doc.summary, description: doc.description, tags: base_tags.clone(), payload: Some(val.clone()) };
                messages_map.insert(resp_id.clone(), resp_message);
                resp_refs.push(RefObj { r#ref: format!("{}/messages/{}", channel_pointer, escape_json_pointer_token(&resp_id)) });
            }
            operations.insert(
                format!("{msg_id}_receive"),
                Operation { action: "receive", channel: RefObj { r#ref: channel_pointer.clone() }, messages: resp_refs, summary: doc.summary, description: doc.description, tags: base_tags.clone() },
            );
        }

        let params: BTreeMap<String, Parameter> = doc.params.iter().map(|(n, t)| (n.clone(), Parameter { schema: t.schema.clone() })).collect();

        channels.insert(doc.path.to_string(), Channel { address: doc.path.to_string(), messages: messages_map, parameters: params });

        operations.insert(
            msg_id.clone(),
            Operation {
                action: "send",
                channel: RefObj { r#ref: channel_pointer.clone() },
                messages: vec![RefObj { r#ref: format!("{}/messages/{}", channel_pointer, escape_json_pointer_token(&msg_id)) }],
                summary: doc.summary,
                description: doc.description,
                tags: base_tags,
            },
        );
    }

    for doc in loop_docs {
        let msg_id: String = doc.path.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();

        let mut response_vals = Vec::new();
        for ts in &doc.responses {
            if is_primitive(ts.name) {
                response_vals.push(wrap_response(ts.schema.clone(), doc.path));
            } else {
                schema_map.entry(ts.name.to_string()).or_insert_with(|| ts.schema.clone());
                response_vals.push(wrap_response(serde_json::json!({"$ref": format!("#/components/schemas/{}", ts.name)}), doc.path));
            }
        }

        let base_tags: Vec<Tag> = doc.tags.iter().cloned().map(|t| Tag { name: t, description: None, external_docs: None }).collect();

        let mut messages_map: BTreeMap<String, Message> = BTreeMap::new();
        let mut refs_vec = Vec::new();

        let channel_pointer = format!("#/channels/{}", escape_json_pointer_token(doc.path));

        for (idx, val) in response_vals.iter().enumerate() {
            let resp_id = if response_vals.len() == 1 { msg_id.clone() } else { format!("{msg_id}_response_{idx}") };
            let resp_message = Message { summary: doc.summary, description: doc.description, tags: base_tags.clone(), payload: Some(val.clone()) };
            messages_map.insert(resp_id.clone(), resp_message);
            refs_vec.push(RefObj { r#ref: format!("{}/messages/{}", channel_pointer, escape_json_pointer_token(&resp_id)) });
        }

        channels.insert(doc.path.to_string(), Channel { address: doc.path.to_string(), messages: messages_map, parameters: BTreeMap::new() });

        operations.insert(
            msg_id.clone(),
            Operation { action: "receive", channel: RefObj { r#ref: channel_pointer }, messages: refs_vec, summary: doc.summary, description: doc.description, tags: base_tags },
        );
    }

    schema_map.retain(|k, _| !is_primitive(k.as_str()));
    info.tags = tags.to_vec();
    info.external_docs = external;

    AsyncApiDoc { asyncapi: "3.0.0", id, info, default_content_type: "application/json", servers, channels, operations, components: Components { schemas: schema_map } }
}
