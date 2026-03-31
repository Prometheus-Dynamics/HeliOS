use std::path::Path;

use super::{LogSource, LogSourceKind};

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
