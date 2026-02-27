use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::process::Command;
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LogSourceKind {
    JournalSystem,
    JournalUnit,
    Dmesg,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema, Default)]
pub struct SystemdUnitStatus {
    pub active_state: Option<String>,
    pub sub_state: Option<String>,
    pub unit_file_state: Option<String>,
    pub description: Option<String>,
    pub fragment_path: Option<String>,
    pub main_pid: Option<u32>,
    pub exec_main_status: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct LogSource {
    pub id: String,
    pub label: String,
    pub group: String,
    pub kind: LogSourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub important: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<SystemdUnitStatus>,
}

pub fn default_log_sources() -> Vec<LogSource> {
    vec![
        LogSource { id: "journal".into(), label: "System journal".into(), group: "System".into(), kind: LogSourceKind::JournalSystem, unit: None, path: None, important: true, status: None },
        LogSource { id: "dmesg".into(), label: "Kernel (dmesg)".into(), group: "System".into(), kind: LogSourceKind::Dmesg, unit: None, path: None, important: true, status: None },
        LogSource {
            id: "unit:helios-api.service".into(),
            label: "helios-api.service".into(),
            group: "Helios".into(),
            kind: LogSourceKind::JournalUnit,
            unit: Some("helios-api.service".into()),
            path: None,
            important: true,
            status: None,
        },
        LogSource {
            id: "unit:helios-engine.service".into(),
            label: "helios-engine.service".into(),
            group: "Helios".into(),
            kind: LogSourceKind::JournalUnit,
            unit: Some("helios-engine.service".into()),
            path: None,
            important: true,
            status: None,
        },
        LogSource {
            id: "unit:helios-peripherals.service".into(),
            label: "helios-peripherals.service".into(),
            group: "Helios".into(),
            kind: LogSourceKind::JournalUnit,
            unit: Some("helios-peripherals.service".into()),
            path: None,
            important: true,
            status: None,
        },
        LogSource {
            id: "unit:helios-updater.service".into(),
            label: "helios-updater.service".into(),
            group: "Helios".into(),
            kind: LogSourceKind::JournalUnit,
            unit: Some("helios-updater.service".into()),
            path: None,
            important: false,
            status: None,
        },
    ]
}

pub fn discover_file_sources() -> Vec<LogSource> {
    let candidates = [
        ("/var/log/syslog", "syslog", "System"),
        ("/var/log/messages", "messages", "System"),
        ("/var/log/kern.log", "kern.log", "System"),
        ("/var/log/auth.log", "auth.log", "Security"),
        ("/var/log/daemon.log", "daemon.log", "System"),
    ];

    candidates
        .iter()
        .filter(|(path, _, _)| Path::new(path).exists())
        .map(|(path, label, group)| LogSource {
            id: format!("file:{path}"),
            label: label.to_string(),
            group: (*group).to_string(),
            kind: LogSourceKind::File,
            unit: None,
            path: Some(path.to_string()),
            important: false,
            status: None,
        })
        .collect()
}

pub async fn hydrate_systemd_statuses(mut sources: Vec<LogSource>) -> Vec<LogSource> {
    let mut updated = Vec::with_capacity(sources.len());
    for mut source in sources.drain(..) {
        if let Some(unit) = source.unit.clone() {
            source.status = query_unit_status(&unit).await;
        }
        updated.push(source);
    }
    updated
}

async fn query_unit_status(unit: &str) -> Option<SystemdUnitStatus> {
    let output =
        Command::new("systemctl").arg("show").arg(unit).arg("--no-pager").arg("--property=ActiveState,SubState,UnitFileState,Description,FragmentPath,MainPID,ExecMainStatus").output().await.ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut status = SystemdUnitStatus::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim();
        let value = (!value.is_empty()).then(|| value.to_string());
        match key {
            "ActiveState" => status.active_state = value,
            "SubState" => status.sub_state = value,
            "UnitFileState" => status.unit_file_state = value,
            "Description" => status.description = value,
            "FragmentPath" => status.fragment_path = value,
            "MainPID" => status.main_pid = value.and_then(|v| v.parse::<u32>().ok()),
            "ExecMainStatus" => status.exec_main_status = value.and_then(|v| v.parse::<i32>().ok()),
            _ => {}
        }
    }
    Some(status)
}
