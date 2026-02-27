use lib_asyncapi::SchemaProvider;
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ConsoleClientEvent {
    Input { data: String },
    Resize { cols: u16, rows: u16 },
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ConsoleServerEvent {
    Ready { session_id: Uuid, shell: String, cols: u16, rows: u16 },
    Output { data: String },
    Exit { code: Option<i32> },
    Error { message: String },
}

fn schema<T: JsonSchema>() -> serde_json::Value {
    serde_json::to_value(schema_for!(T)).expect("schema")
}

impl SchemaProvider for ConsoleClientEvent {
    const NAME: &'static str = "ConsoleClientEvent";
    fn schema() -> serde_json::Value {
        schema::<ConsoleClientEvent>()
    }
    fn register_schemas(map: &mut std::collections::BTreeMap<String, serde_json::Value>) {
        map.entry(Self::NAME.to_string()).or_insert_with(Self::schema);
    }
}

impl SchemaProvider for ConsoleServerEvent {
    const NAME: &'static str = "ConsoleServerEvent";
    fn schema() -> serde_json::Value {
        schema::<ConsoleServerEvent>()
    }
    fn register_schemas(map: &mut std::collections::BTreeMap<String, serde_json::Value>) {
        map.entry(Self::NAME.to_string()).or_insert_with(Self::schema);
    }
}
