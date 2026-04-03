use futures::future::BoxFuture;

pub trait SchemaVersionAccessor {
    fn schema_version(&self) -> Result<Option<u32>, String>;
    fn set_schema_version(&mut self, version: u32) -> Result<(), String>;
}

pub type SyncMigration<T> = fn(T) -> Result<T, String>;
pub type AsyncMigration<T> = fn(T) -> BoxFuture<'static, Result<T, String>>;

#[derive(Clone, Copy)]
pub struct SyncSchemaPlan<T: 'static> {
    document_name: &'static str,
    minimum_supported_version: u32,
    current_version: u32,
    migrations: &'static [(u32, SyncMigration<T>)],
}

#[derive(Clone, Copy)]
pub struct AsyncSchemaPlan<T: 'static> {
    document_name: &'static str,
    minimum_supported_version: u32,
    current_version: u32,
    migrations: &'static [(u32, AsyncMigration<T>)],
}

impl<T: 'static> SyncSchemaPlan<T> {
    pub const fn strict(document_name: &'static str, current_version: u32) -> Self {
        Self { document_name, minimum_supported_version: current_version, current_version, migrations: &[] }
    }

    pub const fn stepwise(document_name: &'static str, minimum_supported_version: u32, current_version: u32, migrations: &'static [(u32, SyncMigration<T>)]) -> Self {
        Self { document_name, minimum_supported_version, current_version, migrations }
    }
}

impl<T: 'static> AsyncSchemaPlan<T> {
    pub const fn strict(document_name: &'static str, current_version: u32) -> Self {
        Self { document_name, minimum_supported_version: current_version, current_version, migrations: &[] }
    }

    pub const fn stepwise(document_name: &'static str, minimum_supported_version: u32, current_version: u32, migrations: &'static [(u32, AsyncMigration<T>)]) -> Self {
        Self { document_name, minimum_supported_version, current_version, migrations }
    }
}

fn validate_plan(document_name: &str, minimum_supported_version: u32, current_version: u32) -> Result<(), String> {
    if minimum_supported_version > current_version {
        return Err(format!("invalid {document_name} schema plan: minimum supported version {minimum_supported_version} exceeds current version {current_version}"));
    }
    Ok(())
}

pub fn normalize_to_current<T>(mut value: T, plan: &SyncSchemaPlan<T>) -> Result<T, String>
where
    T: SchemaVersionAccessor,
{
    validate_plan(plan.document_name, plan.minimum_supported_version, plan.current_version)?;
    let Some(mut version) = value.schema_version()? else {
        return Err(format!("{} is missing required schema_version", plan.document_name));
    };
    if version < plan.minimum_supported_version {
        return Err(format!("unsupported {} schema_version {}; minimum supported version is {}", plan.document_name, version, plan.minimum_supported_version));
    }
    if version > plan.current_version {
        return Err(format!("unsupported {} schema_version {}; current version is {}", plan.document_name, version, plan.current_version));
    }

    while version < plan.current_version {
        let Some((_, migration)) = plan.migrations.iter().find(|(from, _)| *from == version) else {
            return Err(format!("no {} migration registered from schema_version {} to {}", plan.document_name, version, version + 1));
        };
        value = migration(value)?;
        version = value.schema_version()?.unwrap_or(version + 1).max(version + 1);
    }

    value.set_schema_version(plan.current_version)?;
    Ok(value)
}

pub async fn normalize_to_current_async<T>(mut value: T, plan: &AsyncSchemaPlan<T>) -> Result<T, String>
where
    T: SchemaVersionAccessor,
{
    validate_plan(plan.document_name, plan.minimum_supported_version, plan.current_version)?;
    let Some(mut version) = value.schema_version()? else {
        return Err(format!("{} is missing required schema_version", plan.document_name));
    };
    if version < plan.minimum_supported_version {
        return Err(format!("unsupported {} schema_version {}; minimum supported version is {}", plan.document_name, version, plan.minimum_supported_version));
    }
    if version > plan.current_version {
        return Err(format!("unsupported {} schema_version {}; current version is {}", plan.document_name, version, plan.current_version));
    }

    while version < plan.current_version {
        let Some((_, migration)) = plan.migrations.iter().find(|(from, _)| *from == version) else {
            return Err(format!("no {} migration registered from schema_version {} to {}", plan.document_name, version, version + 1));
        };
        value = migration(value).await?;
        version = value.schema_version()?.unwrap_or(version + 1).max(version + 1);
    }

    value.set_schema_version(plan.current_version)?;
    Ok(value)
}

fn version_to_u32(raw: i64, document_name: &str) -> Result<u32, String> {
    u32::try_from(raw).map_err(|_| format!("{document_name} `schema_version` {raw} does not fit in u32"))
}

impl SchemaVersionAccessor for serde_json::Value {
    fn schema_version(&self) -> Result<Option<u32>, String> {
        let Some(object) = self.as_object() else {
            return Err("document must be a JSON object".to_string());
        };
        let Some(version) = object.get("schema_version") else {
            return Ok(None);
        };
        let Some(version) = version.as_u64() else {
            return Err("document `schema_version` must be an unsigned integer".to_string());
        };
        u32::try_from(version).map(Some).map_err(|_| format!("document `schema_version` {version} does not fit in u32"))
    }

    fn set_schema_version(&mut self, version: u32) -> Result<(), String> {
        let Some(object) = self.as_object_mut() else {
            return Err("document must be a JSON object".to_string());
        };
        object.insert("schema_version".to_string(), serde_json::Value::from(version));
        Ok(())
    }
}

impl SchemaVersionAccessor for toml::Value {
    fn schema_version(&self) -> Result<Option<u32>, String> {
        let Some(table) = self.as_table() else {
            return Err("document must be a TOML table".to_string());
        };
        let Some(version) = table.get("schema_version") else {
            return Ok(None);
        };
        let Some(version) = version.as_integer() else {
            return Err("document `schema_version` must be an unsigned integer".to_string());
        };
        version_to_u32(version, "document").map(Some)
    }

    fn set_schema_version(&mut self, version: u32) -> Result<(), String> {
        let Some(table) = self.as_table_mut() else {
            return Err("document must be a TOML table".to_string());
        };
        table.insert("schema_version".to_string(), toml::Value::Integer(i64::from(version)));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use futures::future::BoxFuture;
    use serde_json::json;

    use super::{AsyncMigration, AsyncSchemaPlan, SchemaVersionAccessor, SyncMigration, SyncSchemaPlan, normalize_to_current, normalize_to_current_async};

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct DemoWire {
        schema_version: Option<u32>,
        body: String,
    }

    impl SchemaVersionAccessor for DemoWire {
        fn schema_version(&self) -> Result<Option<u32>, String> {
            Ok(self.schema_version)
        }

        fn set_schema_version(&mut self, version: u32) -> Result<(), String> {
            self.schema_version = Some(version);
            Ok(())
        }
    }

    fn migrate_demo_v0_to_v1(mut wire: DemoWire) -> Result<DemoWire, String> {
        wire.schema_version = Some(1);
        wire.body.push_str("|v1");
        Ok(wire)
    }

    fn migrate_demo_v1_to_v2(mut wire: DemoWire) -> Result<DemoWire, String> {
        wire.schema_version = Some(2);
        wire.body.push_str("|v2");
        Ok(wire)
    }

    fn migrate_demo_json_v0_to_v1(mut value: serde_json::Value) -> Result<serde_json::Value, String> {
        value["body"] = serde_json::Value::from("v1");
        value.set_schema_version(1)?;
        Ok(value)
    }

    fn migrate_demo_toml_v0_to_v1(mut value: toml::Value) -> Result<toml::Value, String> {
        let table = value.as_table_mut().ok_or_else(|| "document must be a TOML table".to_string())?;
        table.insert("body".to_string(), toml::Value::String("v1".to_string()));
        value.set_schema_version(1)?;
        Ok(value)
    }

    fn migrate_demo_async_v0_to_v1(mut wire: DemoWire) -> BoxFuture<'static, Result<DemoWire, String>> {
        Box::pin(async move {
            wire.schema_version = Some(1);
            wire.body.push_str("|async-v1");
            Ok(wire)
        })
    }

    const DEMO_MIGRATIONS: &[(u32, SyncMigration<DemoWire>)] = &[(0, migrate_demo_v0_to_v1), (1, migrate_demo_v1_to_v2)];
    const DEMO_JSON_MIGRATIONS: &[(u32, SyncMigration<serde_json::Value>)] = &[(0, migrate_demo_json_v0_to_v1)];
    const DEMO_TOML_MIGRATIONS: &[(u32, SyncMigration<toml::Value>)] = &[(0, migrate_demo_toml_v0_to_v1)];
    const DEMO_ASYNC_MIGRATIONS: &[(u32, AsyncMigration<DemoWire>)] = &[(0, migrate_demo_async_v0_to_v1)];

    #[test]
    fn normalize_sync_runs_explicit_version_chain() {
        let plan = SyncSchemaPlan::stepwise("demo wire", 0, 2, DEMO_MIGRATIONS);
        let migrated = normalize_to_current(DemoWire { schema_version: Some(0), body: "seed".to_string() }, &plan).expect("normalize");
        assert_eq!(migrated.schema_version, Some(2));
        assert_eq!(migrated.body, "seed|v1|v2");
    }

    #[test]
    fn normalize_sync_rejects_future_versions() {
        let plan = SyncSchemaPlan::stepwise("demo wire", 0, 2, DEMO_MIGRATIONS);
        let err = normalize_to_current(DemoWire { schema_version: Some(3), body: String::new() }, &plan).expect_err("future version should fail");
        assert!(err.contains("unsupported demo wire schema_version"));
    }

    #[test]
    fn normalize_sync_rejects_missing_schema_version() {
        let plan = SyncSchemaPlan::stepwise("demo wire", 0, 2, DEMO_MIGRATIONS);
        let err = normalize_to_current(DemoWire { schema_version: None, body: "seed".to_string() }, &plan).expect_err("missing schema version should fail");
        assert!(err.contains("missing required schema_version"));
    }

    #[test]
    fn normalize_sync_supports_json_values() {
        let plan = SyncSchemaPlan::stepwise("demo json", 0, 1, DEMO_JSON_MIGRATIONS);
        let migrated = normalize_to_current(json!({"schema_version":0,"name":"demo"}), &plan).expect("normalize json");
        assert_eq!(migrated["schema_version"], json!(1));
        assert_eq!(migrated["body"], json!("v1"));
    }

    #[test]
    fn normalize_sync_supports_toml_values() {
        let plan = SyncSchemaPlan::stepwise("demo toml", 0, 1, DEMO_TOML_MIGRATIONS);
        let mut table = toml::map::Map::new();
        table.insert("schema_version".to_string(), toml::Value::Integer(0));
        let value = toml::Value::Table(table);
        let migrated = normalize_to_current(value, &plan).expect("normalize toml");
        assert_eq!(migrated.get("schema_version").and_then(toml::Value::as_integer), Some(1));
        assert_eq!(migrated.get("body").and_then(toml::Value::as_str), Some("v1"));
    }

    #[tokio::test]
    async fn normalize_async_runs_step_chain() {
        let plan = AsyncSchemaPlan::stepwise("demo async", 0, 1, DEMO_ASYNC_MIGRATIONS);
        let migrated = normalize_to_current_async(DemoWire { schema_version: Some(0), body: "seed".to_string() }, &plan).await.expect("normalize async");
        assert_eq!(migrated.schema_version, Some(1));
        assert_eq!(migrated.body, "seed|async-v1");
    }

    #[test]
    fn strict_sync_plan_accepts_only_current_version() {
        let plan = SyncSchemaPlan::<DemoWire>::strict("demo strict", 1);
        let normalized = normalize_to_current(DemoWire { schema_version: Some(1), body: "seed".to_string() }, &plan).expect("strict plan should accept current version");
        assert_eq!(normalized.schema_version, Some(1));
        assert_eq!(normalized.body, "seed");
    }

    #[test]
    fn strict_sync_plan_rejects_older_versions() {
        let plan = SyncSchemaPlan::<DemoWire>::strict("demo strict", 1);
        let err = normalize_to_current(DemoWire { schema_version: Some(0), body: "seed".to_string() }, &plan).expect_err("strict plan should reject older versions");
        assert!(err.contains("minimum supported version is 1"));
    }
}
