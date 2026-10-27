//! Saved maps: the map files a live mapper saves (`POST /v1/pipelines/{id}/map/save`, or the
//! `map/save` NetworkTables trigger) and a pipeline starts from (its `map` setting). A map file is
//! Eidos's (`helios-maps` reads its header); HeliOS keeps the bytes under the layout's data
//! directory, one file per map, with an index of what each is (`index.json`): its name, when it
//! was saved, its size, the pipeline and camera that made it, and the file format's version.
//!
//! The store is bounded: a save or upload past [`MAX_TOTAL_BYTES`] is refused (507) until a map is
//! deleted. Deleting a map a pipeline starts from is refused too (409).

use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::{
    error::{ApiError, ApiResult},
    host::now_ms,
    store::{read_json, write_json},
};

/// The largest map file kept (a map of a few hundred keyframes is about a megabyte).
pub const MAX_MAP_BYTES: usize = 16 * 1024 * 1024;
/// The most the maps may take together.
pub const MAX_TOTAL_BYTES: u64 = 128 * 1024 * 1024;
/// The longest a map's name may be.
pub const MAX_NAME_CHARS: usize = 120;

/// One stored map, as the index keeps it and the API lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapEntry {
    pub id: String,
    pub name: String,
    /// When it was stored, milliseconds since the Unix epoch.
    pub saved_at_ms: u64,
    pub size_bytes: u64,
    /// The pipeline whose mapper saved it (`None` for an uploaded map).
    pub pipeline: Option<String>,
    /// The camera that pipeline took its frames from.
    pub camera: Option<String>,
    /// The map file's version (Eidos's format, `helios_maps::VERSION`).
    pub format_version: u16,
    /// When the mapper created the map, nanoseconds since the Unix epoch (from the file's header).
    pub created_ns: u64,
    /// The software that wrote the map (from the file's header).
    pub creator: String,
}

/// Where a map came from, for its index entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Source {
    pub pipeline: Option<String>,
    pub camera: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    maps: BTreeMap<String, MapEntry>,
}

/// The maps folder: the index and one `<id>.eidmap` file per map.
pub struct MapStore {
    dir: PathBuf,
    /// One writer at a time: the index is read, changed and written back.
    writing: Mutex<()>,
}

impl MapStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into(), writing: Mutex::new(()) }
    }

    /// Where map `id`'s file is (the path a pipeline's `map.path` names).
    pub fn file_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.eidmap"))
    }

    fn index_path(&self) -> PathBuf {
        self.dir.join("index.json")
    }

    async fn index(&self) -> ApiResult<Index> {
        Ok(read_json(&self.index_path()).await?.unwrap_or_default())
    }

    /// Every stored map, newest first.
    pub async fn list(&self) -> ApiResult<Vec<MapEntry>> {
        let mut maps: Vec<MapEntry> = self.index().await?.maps.into_values().collect();
        maps.sort_by(|a, b| b.saved_at_ms.cmp(&a.saved_at_ms).then_with(|| a.id.cmp(&b.id)));
        Ok(maps)
    }

    /// The bytes that are kept for the maps (their total size).
    pub async fn total_bytes(&self) -> ApiResult<u64> {
        Ok(self.index().await?.maps.values().map(|entry| entry.size_bytes).sum())
    }

    /// Map `id`'s entry.
    pub async fn get(&self, id: &str) -> ApiResult<MapEntry> {
        self.index().await?.maps.remove(id).ok_or_else(|| ApiError::not_found(format!("no map {id}")))
    }

    /// Map `id`'s file, as stored.
    pub async fn read(&self, id: &str) -> ApiResult<Vec<u8>> {
        self.get(id).await?;
        tokio::fs::read(self.file_path(id)).await.map_err(|error| ApiError::internal(format!("map {id}: {error}")))
    }

    /// Stores `bytes` (a map file) as a new map named `name` (`None`: named after its id).
    /// Refused unless the bytes are a version-1 map file, at most [`MAX_MAP_BYTES`], and the maps
    /// stay within [`MAX_TOTAL_BYTES`] (`507`).
    pub async fn put(&self, name: Option<&str>, bytes: &[u8], source: Source) -> ApiResult<MapEntry> {
        let header = check_file(bytes)?;
        let name = name.map(str::trim).filter(|name| !name.is_empty()).map(str::to_string);
        if name.as_ref().is_some_and(|name| name.chars().count() > MAX_NAME_CHARS) {
            return Err(ApiError::unprocessable(format!("a map's name is at most {MAX_NAME_CHARS} characters")));
        }
        let _writing = self.writing.lock().await;
        let mut index = self.index().await?;
        let used: u64 = index.maps.values().map(|entry| entry.size_bytes).sum();
        let size = bytes.len() as u64;
        if used + size > MAX_TOTAL_BYTES {
            return Err(ApiError::insufficient_storage(format!("the maps take {used} of {MAX_TOTAL_BYTES} bytes and this map needs {size}: delete a map first")));
        }
        let id = new_id(&index);
        let name = name.unwrap_or_else(|| id.clone());
        tokio::fs::create_dir_all(&self.dir).await.map_err(|error| ApiError::internal(format!("maps folder: {error}")))?;
        let path = self.file_path(&id);
        let temporary = path.with_extension("eidmap.tmp");
        tokio::fs::write(&temporary, bytes).await.map_err(|error| ApiError::internal(format!("map {id}: {error}")))?;
        tokio::fs::rename(&temporary, &path).await.map_err(|error| ApiError::internal(format!("map {id}: {error}")))?;
        let entry = MapEntry {
            id: id.clone(),
            name,
            saved_at_ms: now_ms(),
            size_bytes: size,
            pipeline: source.pipeline,
            camera: source.camera,
            format_version: header.version,
            created_ns: header.created_ns,
            creator: header.creator,
        };
        index.maps.insert(id, entry.clone());
        if let Err(error) = write_json(&self.index_path(), &index).await {
            let _ = tokio::fs::remove_file(&path).await;
            return Err(error);
        }
        Ok(entry)
    }

    /// Forgets map `id` and its file. The caller checks no pipeline starts from it.
    pub async fn delete(&self, id: &str) -> ApiResult<()> {
        let _writing = self.writing.lock().await;
        let mut index = self.index().await?;
        if index.maps.remove(id).is_none() {
            return Err(ApiError::not_found(format!("no map {id}")));
        }
        write_json(&self.index_path(), &index).await?;
        match tokio::fs::remove_file(self.file_path(id)).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(ApiError::internal(format!("map {id}: {error}"))),
        }
    }
}

/// The header of `bytes` when they are a map file this API keeps; otherwise why not.
pub fn check_file(bytes: &[u8]) -> ApiResult<helios_maps::Header> {
    if bytes.len() > MAX_MAP_BYTES {
        return Err(ApiError::unprocessable(format!("a map is at most {MAX_MAP_BYTES} bytes, this one is {}", bytes.len())));
    }
    helios_maps::parse_header(bytes).map_err(|error| ApiError::unprocessable(format!("not a saved map: {error}")))
}

/// A new map id (`map-` and 12 hex digits, random), not in `index`.
fn new_id(index: &Index) -> String {
    loop {
        let mut bytes = [0_u8; 6];
        getrandom::fill(&mut bytes).expect("OS randomness");
        let id = format!("map-{}", hex::encode(bytes));
        if !index.maps.contains_key(&id) {
            return id;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    /// A version-1 map file with `creator` and no map after the header.
    fn map_file(creator: &[u8]) -> Vec<u8> {
        let mut bytes = helios_maps::MAGIC.to_vec();
        bytes.extend(1_u16.to_le_bytes());
        bytes.extend(0_u16.to_le_bytes());
        bytes.extend(42_u64.to_le_bytes());
        bytes.extend((creator.len() as u16).to_le_bytes());
        bytes.extend(creator);
        bytes.extend([0_u8; 64]);
        bytes
    }

    fn store() -> (tempfile::TempDir, MapStore) {
        let dir = tempfile::tempdir().expect("a directory");
        let store = MapStore::new(dir.path().join("maps"));
        (dir, store)
    }

    #[test]
    fn only_version_one_map_files_are_kept() {
        let header = check_file(&map_file(b"eidos-mapping")).expect("a map");
        assert_eq!((header.version, header.created_ns, header.creator.as_str()), (1, 42, "eidos-mapping"));
        assert!(check_file(b"not a map").is_err());
        let mut newer = map_file(b"x");
        newer[8] = 2;
        assert!(check_file(&newer).is_err(), "version 2 is refused");
        assert!(check_file(&vec![0_u8; MAX_MAP_BYTES + 1]).is_err(), "over the cap is refused");
    }

    #[tokio::test]
    async fn a_stored_map_is_listed_read_and_survives_a_new_store() {
        let (dir, store) = store();
        let bytes = map_file(b"eidos-mapping");
        let saved = store.put(Some("  Match 12 "), &bytes, Source { pipeline: Some("live".into()), camera: Some("camera.a".into()) }).await.expect("put");
        assert!(saved.id.starts_with("map-") && saved.id.len() == 16, "{}", saved.id);
        assert_eq!(saved.name, "Match 12", "the name is trimmed");
        assert_eq!((saved.size_bytes, saved.format_version, saved.creator.as_str()), (bytes.len() as u64, 1, "eidos-mapping"));
        assert_eq!(store.read(&saved.id).await.expect("read"), bytes, "the bytes come back as stored");

        let reopened = MapStore::new(dir.path().join("maps"));
        assert_eq!(reopened.list().await.expect("list"), vec![saved.clone()]);
        assert_eq!(reopened.get(&saved.id).await.expect("get").pipeline.as_deref(), Some("live"));
        assert_eq!(reopened.total_bytes().await.expect("total"), bytes.len() as u64);
    }

    #[tokio::test]
    async fn an_unnamed_map_is_named_after_its_id_and_lists_newest_first() {
        let (_dir, store) = store();
        let first = store.put(None, &map_file(b"a"), Source::default()).await.expect("first");
        assert_eq!(first.name, first.id);
        let second = store.put(Some("second"), &map_file(b"b"), Source::default()).await.expect("second");
        let listed: Vec<String> = store.list().await.expect("list").into_iter().map(|entry| entry.id).collect();
        assert_eq!(listed.len(), 2);
        assert!(listed.contains(&second.id) && listed.contains(&first.id));
        assert!(second.saved_at_ms >= first.saved_at_ms);
    }

    #[tokio::test]
    async fn a_bad_file_or_name_is_refused_and_stores_nothing() {
        let (_dir, store) = store();
        assert!(store.put(None, b"EIDMAP\0\0 truncated", Source::default()).await.is_err());
        let long = "x".repeat(MAX_NAME_CHARS + 1);
        assert!(store.put(Some(&long), &map_file(b"a"), Source::default()).await.is_err());
        assert!(store.list().await.expect("list").is_empty());
    }

    #[tokio::test]
    async fn the_store_refuses_a_map_past_its_total_cap() {
        let (_dir, store) = store();
        // Fill the index to the cap without writing megabytes: an entry of the cap's size.
        let fill = MapEntry {
            id: "map-filled".into(),
            name: "filled".into(),
            saved_at_ms: 1,
            size_bytes: MAX_TOTAL_BYTES - 10,
            pipeline: None,
            camera: None,
            format_version: 1,
            created_ns: 0,
            creator: String::new(),
        };
        tokio::fs::create_dir_all(&store.dir).await.expect("dir");
        write_json(&store.index_path(), &Index { maps: BTreeMap::from([(fill.id.clone(), fill)]) }).await.expect("index");
        let error = store.put(None, &map_file(b"a"), Source::default()).await.expect_err("past the cap");
        assert_eq!(error.code, ErrorCode::InsufficientStorage, "{error:?}");
        assert_eq!(store.list().await.expect("list").len(), 1, "nothing was added");
    }

    #[tokio::test]
    async fn deleting_forgets_the_map_and_its_file() {
        let (_dir, store) = store();
        let saved = store.put(None, &map_file(b"a"), Source::default()).await.expect("put");
        store.delete(&saved.id).await.expect("delete");
        assert!(store.get(&saved.id).await.is_err());
        assert!(!store.file_path(&saved.id).exists(), "the file is gone");
        assert!(store.delete(&saved.id).await.is_err(), "deleting twice is not found");
    }
}
