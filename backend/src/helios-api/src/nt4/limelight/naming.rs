use std::collections::BTreeSet;

use helios_engine::ipc::{StreamState, StreamSummary};

#[derive(Debug, Clone)]
pub(super) struct AdapterSeed {
    pub(super) table_name: String,
    pub(super) stream_id: uuid::Uuid,
    pub(super) stream_alias: Option<String>,
    pub(super) stream_state: String,
    pub(super) recording_active: bool,
}

pub(super) fn build_adapter_seeds(streams: Vec<StreamSummary>) -> Vec<AdapterSeed> {
    let mut visible = streams.into_iter().filter(|stream| !stream.manifest.internal).collect::<Vec<_>>();
    visible.sort_by(|a, b| {
        let a_key = candidate_table_base(a);
        let b_key = candidate_table_base(b);
        a_key.cmp(&b_key).then_with(|| a.stream_id.cmp(&b.stream_id))
    });

    let mut seen_tables = BTreeSet::new();
    let mut out = Vec::with_capacity(visible.len());
    for stream in visible {
        let base = candidate_table_base(&stream);
        let table_name = unique_table_name(&base, &mut seen_tables);
        let stream_state = match stream.status.state {
            StreamState::Running => "running",
            StreamState::Disabled => "disabled",
        }
        .to_string();

        out.push(AdapterSeed {
            table_name,
            stream_id: stream.stream_id,
            stream_alias: stream.manifest.identity.alias.clone().and_then(non_empty_trimmed),
            stream_state,
            recording_active: stream.status.recording_active,
        });
    }
    out
}

pub(super) fn sanitize_table_name(raw: &str, fallback: &str) -> String {
    let trimmed = raw.trim();
    let mut out = String::new();
    let mut previous_dash = false;
    for ch in trimmed.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            previous_dash = false;
        } else if (c == '-' || c == '_' || c == ' ' || c == '.' || c == '/') && !previous_dash {
            out.push('-');
            previous_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() { fallback.to_string() } else { out }
}

pub(super) fn unique_table_name(base: &str, seen: &mut BTreeSet<String>) -> String {
    if seen.insert(base.to_string()) {
        return base.to_string();
    }
    let mut suffix = 2usize;
    loop {
        let candidate = format!("{base}-{suffix}");
        if seen.insert(candidate.clone()) {
            return candidate;
        }
        suffix += 1;
    }
}

fn candidate_table_base(stream: &StreamSummary) -> String {
    if let Some(alias) = stream.manifest.identity.alias.as_deref()
        && !alias.trim().is_empty()
    {
        return sanitize_table_name(alias, "limelight");
    }
    if let Some(hardware_id) = stream.manifest.identity.hardware_id.as_deref()
        && !hardware_id.trim().is_empty()
    {
        return sanitize_table_name(hardware_id, "limelight");
    }
    sanitize_table_name(&stream.stream_id.to_string(), "limelight")
}

fn non_empty_trimmed(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}
