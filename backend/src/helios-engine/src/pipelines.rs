use serde_json::Value as JsonValue;
use std::io;
use std::path::PathBuf;
use std::sync::OnceLock;
use uuid::Uuid;

static DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();

fn data_root() -> PathBuf {
    DATA_ROOT
        .get_or_init(|| {
            if let Ok(dir) = std::env::var("HELIOS_PIPELINE_DIR") {
                return PathBuf::from(dir);
            }
            if let Ok(dir) = std::env::var("HELIOS_API_DATA_DIR") {
                return PathBuf::from(dir);
            }

            let candidates = [PathBuf::from("/data/helios/api"), PathBuf::from("/var/lib/helios/api"), std::env::temp_dir().join("helios-api")];

            for candidate in candidates {
                if candidate.is_dir() || std::fs::create_dir_all(&candidate).is_ok() {
                    return candidate;
                }
            }

            std::env::temp_dir().join("helios-api")
        })
        .clone()
}

fn pipeline_dir() -> PathBuf {
    data_root().join("pipelines")
}

fn template_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("HELIOS_PIPELINE_TEMPLATE_DIR") {
        return PathBuf::from(dir);
    }
    if let Ok(cwd) = std::env::current_dir() {
        let dev = cwd.join("configs").join("templates");
        if dev.is_dir() {
            return dev;
        }
    }
    PathBuf::from("/usr/share/helios/pipeline-templates")
}

pub(crate) fn load_pipeline_graph_json(pipeline_id: Uuid) -> io::Result<JsonValue> {
    let path = pipeline_dir().join(format!("{pipeline_id}.json"));
    let data = std::fs::read(&path)?;
    let doc = serde_json::from_slice::<JsonValue>(&data).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let mut graph = doc.get("graph").cloned().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "pipeline graph missing 'graph' field"))?;
    if let Some(unwrapped) = unwrap_pipeline_export_graph(&graph) {
        graph = unwrapped;
    }
    Ok(graph)
}

fn unwrap_pipeline_export_graph(payload: &JsonValue) -> Option<JsonValue> {
    let obj = payload.as_object()?;
    if let Some(graph) = obj.get("graph").and_then(|g| g.as_object()).cloned() {
        return Some(JsonValue::Object(graph));
    }
    let pipeline = obj.get("pipeline")?.as_object()?;
    let graph = pipeline.get("graph")?.as_object()?.clone();
    Some(JsonValue::Object(graph))
}

pub fn load_template_graph_json(template_id: &str) -> io::Result<JsonValue> {
    let path = template_dir().join(format!("{template_id}.json"));
    let data = std::fs::read(&path)?;
    let doc = serde_json::from_slice::<JsonValue>(&data).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let graph = doc.get("graph").cloned().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "template graph missing 'graph' field"))?;
    Ok(graph)
}
