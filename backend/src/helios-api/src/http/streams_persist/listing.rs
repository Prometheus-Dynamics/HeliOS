use super::*;
use tokio::sync::OnceCell;

pub(super) async fn list_records() -> Vec<PersistedStreamRecord> {
    let dir = match records_dir().await {
        Ok(dir) => dir,
        Err(_) => return Vec::new(),
    };

    let mut out = Vec::new();
    let mut entries = match fs::read_dir(&dir).await {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let is_file = entry.file_type().await.map(|ft| ft.is_file()).unwrap_or(false);
        if !is_file {
            continue;
        }
        let Ok(bytes) = fs::read(&path).await else {
            continue;
        };
        let Ok(parsed) = parse_persisted_stream_record(&bytes).await else {
            continue;
        };
        let record = parsed.record;
        let mut dirty = parsed.dirty;
        if record.camera_id.is_empty() {
            dirty = true;
        }
        if dirty {
            let _ = json_store::write_json(path.clone(), &record).await;
        }
        out.push(record);
    }

    out
}

async fn migrate_file_camera_ids_once() {
    // Historical bug: File streams were sometimes persisted under a generic device key like
    // `media-file`, which collides across streams and creates duplicate persisted records for the
    // same stream UUID/alias. Canonicalize file streams to be stored under their alias.
    let records = list_records().await;
    for record in records {
        let Some(manifest) = record.requested_manifest() else {
            continue;
        };
        if manifest.internal {
            continue;
        }
        if manifest.capture.backend != styx::BackendKind::File {
            continue;
        }
        let Some(alias) = manifest.identity.alias.clone() else {
            continue;
        };
        if alias == record.camera_id {
            continue;
        }

        let stream_id = record.stream_id().unwrap_or_else(|| derived_stream_id(&record.camera_id));
        persist_manifest_quick(&alias, Some(stream_id), manifest.clone()).await;

        if let Ok(path) = record_path(&record.camera_id).await {
            let _ = fs::remove_file(path).await;
        }
    }
}

pub(super) async fn list_persisted_records() -> Vec<PersistedStreamRecord> {
    static MIGRATED: OnceCell<()> = OnceCell::const_new();
    MIGRATED.get_or_init(|| async { migrate_file_camera_ids_once().await }).await;
    list_records().await
}
