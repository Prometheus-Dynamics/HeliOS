use tokio::process::Command;

use super::{LogSource, SystemdUnitStatus};

pub async fn hydrate_systemd_statuses(mut sources: Vec<LogSource>) -> Vec<LogSource> {
    let mut tasks = tokio::task::JoinSet::new();
    for (idx, source) in sources.iter().enumerate() {
        let Some(unit) = source.unit.clone() else {
            continue;
        };
        tasks.spawn(async move { (idx, query_unit_status(&unit).await) });
    }

    while let Some(result) = tasks.join_next().await {
        let Ok((idx, status)) = result else {
            continue;
        };
        if let Some(source) = sources.get_mut(idx) {
            source.status = status;
        }
    }

    sources
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
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
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
