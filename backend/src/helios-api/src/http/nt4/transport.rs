use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

pub(super) fn team_number_to_rio_ip(team: u32) -> Option<Ipv4Addr> {
    crate::nt4::support::team_number_to_rio_ip(team)
}

pub(super) fn team_number_from_rio_ip(ip: Ipv4Addr) -> Option<u32> {
    let [a0, a1, a2, a3] = ip.octets();
    if a0 != 10 || a3 != 2 || (a1 == 0 && a2 == 0) {
        return None;
    }
    let team = (a1 as u32) * 100 + (a2 as u32);
    if team == 0 || team > 25_599 {
        return None;
    }
    Some(team)
}

pub(super) fn team_number_from_rio_hostname(host: &str) -> Option<u32> {
    let normalized = host.trim().trim_end_matches('.').to_ascii_lowercase();
    let remainder = normalized.strip_prefix("roborio-")?;
    let team_raw = remainder.strip_suffix("-frc.local").or_else(|| remainder.strip_suffix("-frc"))?;
    let team: u32 = team_raw.parse().ok()?;
    if team == 0 || team > 25_599 {
        return None;
    }
    Some(team)
}

pub(super) fn host_candidates(host: &str) -> Vec<String> {
    let trimmed = host.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(4);
    out.push(trimmed.to_string());

    let team = if let Ok(ip) = trimmed.parse::<Ipv4Addr>() { team_number_from_rio_ip(ip) } else { team_number_from_rio_hostname(trimmed) };

    if let Some(team) = team {
        out.push(format!("roborio-{team}-frc.local"));
        if let Some(rio_ip) = team_number_to_rio_ip(team) {
            out.push(rio_ip.to_string());
        }
        out.push("172.22.11.2".to_string());
    }

    let mut deduped = Vec::with_capacity(out.len());
    for candidate in out {
        if !deduped.iter().any(|entry| entry == &candidate) {
            deduped.push(candidate);
        }
    }
    deduped
}

pub(super) async fn connect_nt4_target(requested_host: &str, port: u16, timeout_ms: u64) -> Result<(Arc<crate::nt4::pool::Nt4ClientEntry>, String), String> {
    let candidates = host_candidates(requested_host);
    if candidates.is_empty() {
        return Err("host is required".to_string());
    }

    let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms.clamp(150, 15_000));
    let mut attempts = Vec::new();

    for (idx, candidate) in candidates.iter().enumerate() {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let remaining_candidates = (candidates.len() - idx) as u32;
        let per_candidate = std::cmp::max(Duration::from_millis(150), remaining / remaining_candidates);
        let wait = std::cmp::min(remaining, per_candidate);

        let entry = match crate::nt4::pool().get_or_connect(candidate, port, "HeliOS-nt4").await {
            Ok(entry) => entry,
            Err(err) => {
                attempts.push(format!("{candidate}: {err}"));
                continue;
            }
        };

        match entry.wait_ready(wait).await {
            Ok(()) => return Ok((entry, candidate.clone())),
            Err(err) => {
                attempts.push(format!("{candidate}: {err}"));
                let _ = crate::nt4::pool().disconnect(candidate, port).await;
            }
        }
    }

    if attempts.is_empty() {
        return Err(format!("nt4 connection timed out for host {requested_host}:{port}"));
    }
    Err(format!("unable to connect to nt4 at {requested_host}:{port} (tried: {})", attempts.join("; ")))
}
