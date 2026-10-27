//! Saved maps as HeliOS keeps them: the header of Eidos's map file (`docs/map-format.md` in Eidos,
//! version 1), which helios-api validates on upload and lists, and the host ports a live-map graph
//! takes a map through, which helios-api adds to a deployed graph and helios-engine feeds.
//!
//! The live mapper (`eidos:mapping.live_map`) takes a saved map on its `map` input and saves one on
//! the rising edge of `save`, giving `saved_map`. The graphs Eidos generates leave those ports out,
//! so HeliOS wires them ([`wire_map_ports`]) where a graph is deployed.

use serde_json::{Value, json};

/// The live mapper node (`eidos:mapping.live_map`).
pub const LIVE_MAP_NODE: &str = "eidos:mapping.live_map";
/// The host bridge node: its outputs are the host's inputs to the graph, its inputs the graph's
/// outputs to the host.
pub const HOST_NODE: &str = "io.host_bridge";
/// The host output a saved map is pushed into (the mapper's `map`, `eidos:bytes`), held: it stays
/// until the host pushes another.
pub const MAP_PORT: &str = "map";
/// The host output that asks for a save (the mapper's `save`, a bool; saved on its rising edge).
pub const SAVE_PORT: &str = "save";
/// The host input that gets a save's map file (the mapper's `saved_map`, `eidos:bytes`).
pub const SAVED_MAP_PORT: &str = "saved_map";
/// The map file's magic: `EIDMAP` and two zero bytes.
pub const MAGIC: [u8; 8] = *b"EIDMAP\0\0";
/// The map file version this reads.
pub const VERSION: u16 = 1;

const MAX_CREATOR: usize = 256;
const HOST_HELD_INPUTS: &str = "daedalus.host_held_inputs";
const HOST_INPUT_TYPES: &str = "daedalus.host_input_types";
/// The map input's type (`eidos:bytes`, opaque, as the mapper declares it).
const MAP_TYPE: &str = r#"{"Opaque":"eidos:bytes"}"#;
/// The save input's type.
const SAVE_TYPE: &str = r#"{"Scalar":"Bool"}"#;

/// What a map file says about itself, from its header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// The file's version (1).
    pub version: u16,
    /// When the map was created, nanoseconds since the Unix epoch.
    pub created_ns: u64,
    /// The software that wrote it, e.g. `eidos-mapping 1.0.1-dev`.
    pub creator: String,
}

/// Why bytes are not a map file this reads.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HeaderError {
    #[error("not a saved map (the file does not start with EIDMAP)")]
    BadMagic,
    #[error("map file version {0} is not supported (this reads version {VERSION})")]
    UnsupportedVersion(u16),
    #[error("map file flags {0:#x} are not supported (version {VERSION} has none)")]
    Flags(u16),
    #[error("map file is truncated")]
    Truncated,
    #[error("map file creator is not UTF-8 text of at most {MAX_CREATOR} bytes")]
    Creator,
}

/// The header of a map file: its magic, version and flags are checked, the creator read. The map
/// itself is not parsed here (Eidos's loader does that when a run starts from it).
pub fn parse_header(bytes: &[u8]) -> Result<Header, HeaderError> {
    if bytes.len() < MAGIC.len() || bytes[..MAGIC.len()] != MAGIC {
        return Err(HeaderError::BadMagic);
    }
    let mut cursor = Cursor { bytes, at: MAGIC.len() };
    let version = cursor.u16()?;
    if version != VERSION {
        return Err(HeaderError::UnsupportedVersion(version));
    }
    let flags = cursor.u16()?;
    if flags != 0 {
        return Err(HeaderError::Flags(flags));
    }
    let created_ns = cursor.u64()?;
    let length = usize::from(cursor.u16()?);
    if length > MAX_CREATOR {
        return Err(HeaderError::Creator);
    }
    let creator = std::str::from_utf8(cursor.take(length)?).map_err(|_| HeaderError::Creator)?.to_string();
    Ok(Header { version, created_ns, creator })
}

/// Reads little-endian fields from a map file's header.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], HeaderError> {
        let end = self.at.checked_add(count).filter(|end| *end <= self.bytes.len()).ok_or(HeaderError::Truncated)?;
        let field = &self.bytes[self.at..end];
        self.at = end;
        Ok(field)
    }

    fn u16(&mut self) -> Result<u16, HeaderError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().expect("two bytes")))
    }

    fn u64(&mut self) -> Result<u64, HeaderError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("eight bytes")))
    }
}

/// Whether `graph` (a `GraphDocument` as JSON) has a live mapper, which takes a saved map.
pub fn has_live_map(graph: &Value) -> bool {
    graph.pointer("/graph/nodes").and_then(Value::as_array).is_some_and(|nodes| nodes.iter().any(|node| node.get("id").and_then(Value::as_str) == Some(LIVE_MAP_NODE)))
}

/// Adds a saved map's host ports to a live-map graph document (`GraphDocument` JSON): `map` and
/// `save` become held host outputs (the host pushes them) and `saved_map` a host input, with their
/// edges to the mapper. Idempotent: a graph that has them is unchanged. `false` when the graph has
/// no live mapper or no host bridge (nothing to wire).
pub fn wire_map_ports(graph: &mut Value) -> bool {
    let Some(nodes) = graph.pointer("/graph/nodes").and_then(Value::as_array) else { return false };
    let position = |id: &str| nodes.iter().position(|node| node.get("id").and_then(Value::as_str) == Some(id));
    let (Some(host), Some(live)) = (position(HOST_NODE), position(LIVE_MAP_NODE)) else { return false };
    let nodes = graph.pointer_mut("/graph/nodes").and_then(Value::as_array_mut).expect("nodes were just read");
    let host_node = &mut nodes[host];
    for port in [MAP_PORT, SAVE_PORT] {
        push_unique(&mut host_node["outputs"], port);
    }
    push_unique(&mut host_node["inputs"], SAVED_MAP_PORT);
    let metadata = &mut host_node["metadata"];
    for port in [MAP_PORT, SAVE_PORT] {
        push_unique_string(metadata_list(metadata, HOST_HELD_INPUTS, "List"), port);
    }
    let types = metadata_list(metadata, HOST_INPUT_TYPES, "Map");
    push_type(types, MAP_PORT, MAP_TYPE);
    push_type(types, SAVE_PORT, SAVE_TYPE);

    let Some(edges) = graph.pointer_mut("/graph/edges").and_then(Value::as_array_mut) else { return false };
    for (from, to) in [((host, MAP_PORT), (live, MAP_PORT)), ((host, SAVE_PORT), (live, SAVE_PORT)), ((live, SAVED_MAP_PORT), (host, SAVED_MAP_PORT))] {
        let from = json!({ "node": from.0, "port": from.1 });
        let to = json!({ "node": to.0, "port": to.1 });
        if !edges.iter().any(|edge| edge["from"] == from && edge["to"] == to) {
            edges.push(json!({ "from": from, "to": to }));
        }
    }
    true
}

/// `value` (a JSON array of port names) with `port` appended, unless it has it.
fn push_unique(value: &mut Value, port: &str) {
    if let Some(list) = value.as_array_mut()
        && !list.iter().any(|entry| entry.as_str() == Some(port))
    {
        list.push(json!(port));
    }
}

/// The `{"type": kind, "value": [...]}` metadata entry `key`, made when missing; its list.
fn metadata_list<'a>(metadata: &'a mut Value, key: &str, kind: &str) -> &'a mut Vec<Value> {
    if !metadata[key]["value"].is_array() {
        metadata[key] = json!({ "type": kind, "value": [] });
    }
    metadata[key]["value"].as_array_mut().expect("a list entry")
}

fn push_unique_string(list: &mut Vec<Value>, port: &str) {
    if !list.iter().any(|entry| entry["value"] == port) {
        list.push(json!({ "type": "String", "value": port }));
    }
}

/// The host input types entry for `port`: a `[port, type]` pair, the type as JSON text.
fn push_type(types: &mut Vec<Value>, port: &str, type_json: &str) {
    if !types.iter().any(|pair| pair[0]["value"] == port) {
        types.push(json!([{ "type": "String", "value": port }, { "type": "String", "value": type_json }]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A map file's header (and no map after it): `creator` as written.
    fn header(version: u16, flags: u16, created_ns: u64, creator: &[u8]) -> Vec<u8> {
        let mut bytes = MAGIC.to_vec();
        bytes.extend(version.to_le_bytes());
        bytes.extend(flags.to_le_bytes());
        bytes.extend(created_ns.to_le_bytes());
        bytes.extend((creator.len() as u16).to_le_bytes());
        bytes.extend(creator);
        bytes
    }

    #[test]
    fn a_version_one_header_reads_its_creator_and_time() {
        let bytes = header(1, 0, 1_700_000_000_000_000_000, b"eidos-mapping 1.0.1-dev");
        let parsed = parse_header(&bytes).expect("a version 1 map");
        assert_eq!(parsed, Header { version: 1, created_ns: 1_700_000_000_000_000_000, creator: "eidos-mapping 1.0.1-dev".into() });
    }

    #[test]
    fn bytes_that_are_not_a_map_are_refused_by_their_magic() {
        assert_eq!(parse_header(b"PNG\x89 not a map at all"), Err(HeaderError::BadMagic));
        assert_eq!(parse_header(b"EIDMAP"), Err(HeaderError::BadMagic), "a short file");
        assert_eq!(parse_header(&[]), Err(HeaderError::BadMagic));
    }

    #[test]
    fn an_unknown_version_is_refused() {
        assert_eq!(parse_header(&header(2, 0, 0, b"x")), Err(HeaderError::UnsupportedVersion(2)));
        assert_eq!(parse_header(&header(0, 0, 0, b"x")), Err(HeaderError::UnsupportedVersion(0)));
    }

    #[test]
    fn flags_version_one_does_not_have_are_refused() {
        assert_eq!(parse_header(&header(1, 1, 0, b"x")), Err(HeaderError::Flags(1)), "bit 0 is compression, not in version 1");
    }

    #[test]
    fn a_truncated_header_is_refused() {
        let full = header(1, 0, 5, b"creator");
        for cut in [9, 11, 13, 19, 21, full.len() - 1] {
            assert_eq!(parse_header(&full[..cut]), Err(HeaderError::Truncated), "cut at {cut}");
        }
    }

    #[test]
    fn a_creator_must_be_short_utf8_text() {
        assert_eq!(parse_header(&header(1, 0, 0, &[0xff, 0xfe])), Err(HeaderError::Creator));
        assert_eq!(parse_header(&header(1, 0, 0, &[b'a'; 257])), Err(HeaderError::Creator));
        assert!(parse_header(&header(1, 0, 0, &[b'a'; 256])).is_ok());
    }

    /// A live-map graph as Eidos generates it: the host bridge, the detector, the mapper.
    fn unwired_live_map() -> Value {
        json!({
            "format": "daedalus.graph",
            "graph": {
                "nodes": [
                    { "id": HOST_NODE, "inputs": ["map_pose"], "outputs": ["frame"], "metadata": {
                        "daedalus.host_held_inputs": { "type": "List", "value": [{ "type": "String", "value": "frame" }] },
                        "daedalus.host_input_types": { "type": "Map", "value": [[{ "type": "String", "value": "frame" }, { "type": "String", "value": "{\"Opaque\":\"styx:framelease\"}" }]] }
                    } },
                    { "id": LIVE_MAP_NODE, "inputs": ["frame", "map", "save"], "outputs": ["pose", "saved_map"] }
                ],
                "edges": [
                    { "from": { "node": 0, "port": "frame" }, "to": { "node": 1, "port": "frame" } },
                    { "from": { "node": 1, "port": "pose" }, "to": { "node": 0, "port": "map_pose" } }
                ]
            },
            "requires": [{ "id": "eidos" }],
            "schema_version": 1
        })
    }

    #[test]
    fn wiring_adds_the_map_ports_once() {
        let mut graph = unwired_live_map();
        assert!(has_live_map(&graph));
        assert!(wire_map_ports(&mut graph));
        let once = graph.clone();
        assert!(wire_map_ports(&mut graph));
        assert_eq!(graph, once, "wiring again changes nothing");
        let host = &graph["graph"]["nodes"][0];
        assert_eq!(host["outputs"], json!(["frame", "map", "save"]));
        assert_eq!(host["inputs"], json!(["map_pose", "saved_map"]));
        assert_eq!(host["metadata"]["daedalus.host_held_inputs"]["value"].as_array().map(Vec::len), Some(3));
        let types = host["metadata"]["daedalus.host_input_types"]["value"].as_array().expect("types");
        assert!(types.iter().any(|pair| pair[0]["value"] == "map" && pair[1]["value"] == MAP_TYPE), "{types:?}");
        assert!(types.iter().any(|pair| pair[0]["value"] == "save" && pair[1]["value"] == SAVE_TYPE), "{types:?}");
        let edges = graph["graph"]["edges"].as_array().expect("edges");
        assert!(edges.contains(&json!({ "from": { "node": 0, "port": "map" }, "to": { "node": 1, "port": "map" } })));
        assert!(edges.contains(&json!({ "from": { "node": 0, "port": "save" }, "to": { "node": 1, "port": "save" } })));
        assert!(edges.contains(&json!({ "from": { "node": 1, "port": "saved_map" }, "to": { "node": 0, "port": "saved_map" } })));
    }

    #[test]
    fn a_graph_without_a_live_map_is_left_alone() {
        let mut graph = json!({ "format": "daedalus.graph", "graph": { "nodes": [{ "id": HOST_NODE, "inputs": [], "outputs": [], "metadata": {} }], "edges": [] } });
        let before = graph.clone();
        assert!(!has_live_map(&graph));
        assert!(!wire_map_ports(&mut graph));
        assert_eq!(graph, before);
    }

    #[test]
    fn the_live_map_goldens_wire_and_stay_wired() {
        for text in [
            include_str!("../../engine/graphs/live-map.graph.json"),
            include_str!("../../engine/graphs/dense-map.graph.json"),
            include_str!("../../engine/graphs/apriltag-36h11-map.graph.json"),
            include_str!("../../engine/graphs/apriltag-36h11-map-dense.graph.json"),
        ] {
            let mut graph: Value = serde_json::from_str(text).expect("a golden graph");
            assert!(wire_map_ports(&mut graph));
            let edges = graph["graph"]["edges"].as_array().expect("edges");
            assert_eq!(edges.iter().filter(|edge| edge["to"]["port"] == SAVE_PORT).count(), 1, "one save edge");
            assert_eq!(edges.iter().filter(|edge| edge["to"]["port"] == MAP_PORT).count(), 1, "one map edge");
        }
    }
}
