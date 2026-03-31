use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::StatusCode;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::time::sleep;
use url::Url;
use uuid::Uuid;

use crate::config::UpdaterConfig;
use crate::error::{Error, Result};

pub(crate) const FRONTEND_BUNDLE_KIND: &str = "frontend-bundle";
pub(crate) const SERVICE_BUNDLE_KIND: &str = "service-bundle";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BundleApplyOutcome {
    pub reboot_required: bool,
    pub tryboot: bool,
    pub restart_updater_after_apply: bool,
}

impl BundleApplyOutcome {
    pub(crate) const fn frontend_bundle() -> Self {
        Self { reboot_required: false, tryboot: false, restart_updater_after_apply: false }
    }

    pub(crate) const fn service_bundle(restart_updater_after_apply: bool) -> Self {
        Self { reboot_required: false, tryboot: false, restart_updater_after_apply }
    }

    pub(crate) const fn disk_image(tryboot: bool) -> Self {
        Self { reboot_required: true, tryboot, restart_updater_after_apply: false }
    }
}

#[derive(Debug, Clone)]
enum PreviousFrontendActivation {
    Missing,
    Symlink(PathBuf),
    RenamedDir(PathBuf),
}

#[derive(Debug, Clone)]
struct FrontendActivation {
    active_path: PathBuf,
    previous: PreviousFrontendActivation,
}

#[derive(Debug, Clone)]
enum PreviousBinaryActivation {
    Missing,
    Symlink(PathBuf),
    RenamedFile(PathBuf),
}

#[derive(Debug, Clone)]
struct ServiceBinaryActivation {
    live_path: PathBuf,
    previous: PreviousBinaryActivation,
}

#[derive(Debug, Clone)]
struct ServiceBundleActivation {
    binaries: Vec<ServiceBinaryActivation>,
}

#[derive(Debug, Clone, Copy)]
struct ServiceBinarySpec {
    name: &'static str,
    restart_unit: Option<&'static str>,
    deferred_restart: bool,
    api_healthcheck: bool,
}

#[derive(Debug, Clone)]
struct ServiceBundleBinary {
    live_path: PathBuf,
    release_path: PathBuf,
}

#[derive(Debug, Clone)]
struct ServiceBundlePlan {
    binaries: Vec<ServiceBundleBinary>,
    restart_units: Vec<&'static str>,
    api_healthcheck: bool,
    restart_updater_after_apply: bool,
}

const SERVICE_BINARY_SPECS: &[ServiceBinarySpec] = &[
    ServiceBinarySpec { name: "helios-engine", restart_unit: Some("helios-engine.service"), deferred_restart: false, api_healthcheck: false },
    ServiceBinarySpec { name: "helios-api", restart_unit: Some("helios-api.service"), deferred_restart: false, api_healthcheck: true },
    ServiceBinarySpec { name: "helios-api-tools", restart_unit: None, deferred_restart: false, api_healthcheck: false },
    ServiceBinarySpec { name: "helios-peripherals", restart_unit: Some("helios-peripherals.service"), deferred_restart: false, api_healthcheck: false },
    ServiceBinarySpec { name: "helios-updater", restart_unit: Some("helios-updater.service"), deferred_restart: true, api_healthcheck: false },
];

fn bundle_kind_matches(kind: Option<&str>, expected: &str) -> bool {
    kind.is_some_and(|value| value.trim().eq_ignore_ascii_case(expected))
}

pub(crate) fn is_frontend_bundle(kind: Option<&str>) -> bool {
    bundle_kind_matches(kind, FRONTEND_BUNDLE_KIND)
}

pub(crate) fn is_service_bundle(kind: Option<&str>) -> bool {
    bundle_kind_matches(kind, SERVICE_BUNDLE_KIND)
}

pub(crate) async fn apply_frontend_bundle(config: &UpdaterConfig, update_id: Uuid, bundle_path: &Path) -> Result<BundleApplyOutcome> {
    ensure_directory(config.frontend_releases_dir()).await?;

    let release_dir = config.frontend_releases_dir().join(update_id.to_string());
    let staging_dir = config.frontend_releases_dir().join(format!(".{}.staging", update_id.simple()));

    remove_if_exists(&staging_dir).await?;
    remove_if_exists(&release_dir).await?;
    ensure_directory(&staging_dir).await?;

    if let Err(err) = extract_bundle_archive(bundle_path, &staging_dir).await {
        let _ = remove_if_exists(&staging_dir).await;
        return Err(err);
    }

    if let Err(err) = normalize_frontend_root(&staging_dir).await {
        let _ = remove_if_exists(&staging_dir).await;
        return Err(err);
    }

    if let Err(err) = validate_frontend_release(&staging_dir).await {
        let _ = remove_if_exists(&staging_dir).await;
        return Err(err);
    }

    fs::rename(&staging_dir, &release_dir).await?;
    let activation = activate_frontend_release(config, &release_dir).await?;

    if let Err(primary_err) = restart_and_verify_frontend(config).await {
        match rollback_frontend_release(&activation).await {
            Ok(()) => {
                let _ = restart_and_verify_frontend(config).await;
            }
            Err(rollback_err) => {
                return Err(Error::InvalidState(format!("frontend bundle activation failed: {primary_err}; rollback failed: {rollback_err}")));
            }
        }
        return Err(primary_err);
    }

    finalize_frontend_activation(&activation).await?;

    Ok(BundleApplyOutcome::frontend_bundle())
}

pub(crate) async fn apply_service_bundle(config: &UpdaterConfig, update_id: Uuid, bundle_path: &Path) -> Result<BundleApplyOutcome> {
    ensure_directory(config.service_releases_dir()).await?;

    let release_dir = config.service_releases_dir().join(update_id.to_string());
    let staging_dir = config.service_releases_dir().join(format!(".{}.staging", update_id.simple()));

    remove_if_exists(&staging_dir).await?;
    remove_if_exists(&release_dir).await?;
    ensure_directory(&staging_dir).await?;

    if let Err(err) = extract_bundle_archive(bundle_path, &staging_dir).await {
        let _ = remove_if_exists(&staging_dir).await;
        return Err(err);
    }

    let bundle_specs = match validate_service_bundle_contents(&staging_dir).await {
        Ok(specs) => specs,
        Err(err) => {
            let _ = remove_if_exists(&staging_dir).await;
            return Err(err);
        }
    };

    fs::rename(&staging_dir, &release_dir).await?;
    let plan = build_service_bundle_plan(config, &release_dir, &bundle_specs);
    let activation = activate_service_bundle_release(config, &plan).await?;

    if let Err(primary_err) = restart_and_verify_service_bundle(config, &plan).await {
        match rollback_service_bundle_release(&activation).await {
            Ok(()) => {
                let _ = restart_and_verify_service_bundle(config, &plan).await;
            }
            Err(rollback_err) => {
                return Err(Error::InvalidState(format!("service bundle activation failed: {primary_err}; rollback failed: {rollback_err}")));
            }
        }
        return Err(primary_err);
    }

    finalize_service_bundle_activation(&activation).await?;

    Ok(BundleApplyOutcome::service_bundle(plan.restart_updater_after_apply))
}

pub(crate) fn trigger_updater_restart_later(config: &UpdaterConfig) -> Result<()> {
    let unit = config.updater_service_unit().trim();
    if unit.is_empty() {
        return Ok(());
    }

    Command::new("systemctl").arg("restart").arg(unit).spawn()?;
    Ok(())
}

fn service_binary_spec(name: &str) -> Option<&'static ServiceBinarySpec> {
    SERVICE_BINARY_SPECS.iter().find(|spec| spec.name == name)
}

async fn activate_frontend_release(config: &UpdaterConfig, release_dir: &Path) -> Result<FrontendActivation> {
    let active_path = config.frontend_active_path().to_path_buf();
    let previous = capture_previous_frontend_activation(config.frontend_releases_dir(), &active_path).await?;
    replace_active_path_with_symlink(&active_path, release_dir).await?;

    Ok(FrontendActivation { active_path, previous })
}

async fn rollback_frontend_release(activation: &FrontendActivation) -> Result<()> {
    match &activation.previous {
        PreviousFrontendActivation::Missing => {
            remove_if_exists(&activation.active_path).await?;
        }
        PreviousFrontendActivation::Symlink(previous_target) => {
            replace_active_path_with_symlink(&activation.active_path, previous_target).await?;
        }
        PreviousFrontendActivation::RenamedDir(previous_dir) => {
            remove_if_exists(&activation.active_path).await?;
            fs::rename(previous_dir, &activation.active_path).await?;
        }
    }
    Ok(())
}

async fn finalize_frontend_activation(activation: &FrontendActivation) -> Result<()> {
    if let PreviousFrontendActivation::RenamedDir(previous_dir) = &activation.previous {
        remove_if_exists(previous_dir).await?;
    }
    Ok(())
}

async fn activate_service_bundle_release(config: &UpdaterConfig, plan: &ServiceBundlePlan) -> Result<ServiceBundleActivation> {
    let mut activations = Vec::with_capacity(plan.binaries.len());
    for binary in &plan.binaries {
        let previous = capture_previous_binary_activation(config.service_releases_dir(), &binary.live_path).await?;
        replace_active_path_with_symlink(&binary.live_path, &binary.release_path).await?;
        activations.push(ServiceBinaryActivation { live_path: binary.live_path.clone(), previous });
    }

    Ok(ServiceBundleActivation { binaries: activations })
}

async fn rollback_service_bundle_release(activation: &ServiceBundleActivation) -> Result<()> {
    for binary in activation.binaries.iter().rev() {
        match &binary.previous {
            PreviousBinaryActivation::Missing => {
                remove_if_exists(&binary.live_path).await?;
            }
            PreviousBinaryActivation::Symlink(previous_target) => {
                replace_active_path_with_symlink(&binary.live_path, previous_target).await?;
            }
            PreviousBinaryActivation::RenamedFile(previous_path) => {
                remove_if_exists(&binary.live_path).await?;
                fs::rename(previous_path, &binary.live_path).await?;
            }
        }
    }
    Ok(())
}

async fn finalize_service_bundle_activation(activation: &ServiceBundleActivation) -> Result<()> {
    for binary in &activation.binaries {
        if let PreviousBinaryActivation::RenamedFile(previous_path) = &binary.previous {
            remove_if_exists(previous_path).await?;
        }
    }
    Ok(())
}

async fn capture_previous_frontend_activation(releases_dir: &Path, active_path: &Path) -> Result<PreviousFrontendActivation> {
    let meta = match fs::symlink_metadata(active_path).await {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PreviousFrontendActivation::Missing);
        }
        Err(err) => return Err(Error::Io(err)),
    };

    if meta.file_type().is_symlink() {
        return Ok(PreviousFrontendActivation::Symlink(fs::read_link(active_path).await?));
    }

    let backup_dir = releases_dir.join(format!(".legacy-{}", Uuid::new_v4().simple()));
    fs::rename(active_path, &backup_dir).await?;
    Ok(PreviousFrontendActivation::RenamedDir(backup_dir))
}

async fn capture_previous_binary_activation(releases_dir: &Path, live_path: &Path) -> Result<PreviousBinaryActivation> {
    let meta = match fs::symlink_metadata(live_path).await {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PreviousBinaryActivation::Missing);
        }
        Err(err) => return Err(Error::Io(err)),
    };

    if meta.file_type().is_symlink() {
        return Ok(PreviousBinaryActivation::Symlink(fs::read_link(live_path).await?));
    }
    if meta.is_dir() {
        return Err(Error::InvalidState(format!("service binary target {} is unexpectedly a directory", live_path.display())));
    }

    let backup_path = releases_dir.join(format!(".legacy-{}-{}", Uuid::new_v4().simple(), live_path.file_name().and_then(|value| value.to_str()).unwrap_or("binary")));
    fs::rename(live_path, &backup_path).await?;
    Ok(PreviousBinaryActivation::RenamedFile(backup_path))
}

async fn extract_bundle_archive(bundle_path: &Path, output_dir: &Path) -> Result<()> {
    let status = Command::new("tar").arg("--no-same-owner").arg("--no-same-permissions").arg("-xf").arg(bundle_path).arg("-C").arg(output_dir).status().await?;
    if status.success() { Ok(()) } else { Err(Error::InvalidState(format!("failed to extract bundle {}", bundle_path.display()))) }
}

async fn normalize_frontend_root(root: &Path) -> Result<()> {
    if fs::metadata(root.join("index.html")).await.is_ok() {
        return Ok(());
    }

    let mut entries = fs::read_dir(root).await?;
    let mut top_level = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        top_level.push(entry.path());
    }

    if top_level.len() != 1 {
        return Ok(());
    }

    let nested = &top_level[0];
    let nested_meta = fs::metadata(nested).await?;
    if !nested_meta.is_dir() || fs::metadata(nested.join("index.html")).await.is_err() {
        return Ok(());
    }

    let temp_root = root.with_extension(format!("flatten-{}", Uuid::new_v4().simple()));
    remove_if_exists(&temp_root).await?;
    fs::rename(nested, &temp_root).await?;

    let mut nested_entries = fs::read_dir(&temp_root).await?;
    while let Some(entry) = nested_entries.next_entry().await? {
        let from = entry.path();
        let to = root.join(entry.file_name());
        fs::rename(from, to).await?;
    }
    fs::remove_dir_all(&temp_root).await?;
    Ok(())
}

async fn validate_frontend_release(root: &Path) -> Result<()> {
    if fs::metadata(root.join("index.html")).await.is_err() {
        return Err(Error::InvalidState(format!("frontend bundle {} does not contain index.html at its root", root.display())));
    }
    Ok(())
}

async fn validate_service_bundle_contents(root: &Path) -> Result<Vec<&'static ServiceBinarySpec>> {
    let mut entries = fs::read_dir(root).await?;
    let mut specs = Vec::new();

    while let Some(entry) = entries.next_entry().await? {
        let file_type = entry.file_type().await?;
        if !file_type.is_file() {
            return Err(Error::InvalidState(format!("service bundle {} contains unsupported entry {}", root.display(), entry.path().display())));
        }

        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        let spec = service_binary_spec(&file_name).ok_or_else(|| Error::InvalidState(format!("service bundle {} contains unsupported binary {}", root.display(), file_name)))?;
        validate_service_bundle_binary(&entry.path()).await?;
        specs.push(spec);
    }

    if specs.is_empty() {
        return Err(Error::InvalidState(format!("service bundle {} does not contain any managed binaries", root.display())));
    }

    specs.sort_by_key(|spec| SERVICE_BINARY_SPECS.iter().position(|candidate| candidate.name == spec.name).unwrap_or(usize::MAX));
    specs.dedup_by_key(|spec| spec.name);
    Ok(specs)
}

fn build_service_bundle_plan(config: &UpdaterConfig, release_dir: &Path, specs: &[&'static ServiceBinarySpec]) -> ServiceBundlePlan {
    let binaries = specs.iter().map(|spec| ServiceBundleBinary { live_path: config.service_bin_dir().join(spec.name), release_path: release_dir.join(spec.name) }).collect::<Vec<_>>();

    let mut restart_units = Vec::new();
    let mut api_healthcheck = false;
    let mut restart_updater_after_apply = false;
    for spec in specs {
        if let Some(unit) = spec.restart_unit
            && !spec.deferred_restart
        {
            restart_units.push(unit);
        }
        api_healthcheck |= spec.api_healthcheck;
        restart_updater_after_apply |= spec.deferred_restart;
    }

    ServiceBundlePlan { binaries, restart_units, api_healthcheck, restart_updater_after_apply }
}

async fn validate_service_bundle_binary(path: &Path) -> Result<()> {
    let mut file = fs::File::open(path).await?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).await?;
    if magic == [0x7f, b'E', b'L', b'F'] { Ok(()) } else { Err(Error::InvalidState(format!("service bundle binary {} is not an ELF executable", path.display()))) }
}

async fn restart_and_verify_frontend(config: &UpdaterConfig) -> Result<()> {
    let unit = config.frontend_service_unit().trim();
    if !unit.is_empty() {
        restart_systemd_unit(unit, "frontend activation").await?;
        wait_for_active_unit(unit).await?;
    }

    if let Some(url) = config.frontend_healthcheck_url() {
        wait_for_healthcheck_url(url).await?;
    }

    Ok(())
}

async fn restart_and_verify_service_bundle(config: &UpdaterConfig, plan: &ServiceBundlePlan) -> Result<()> {
    for unit in &plan.restart_units {
        restart_systemd_unit(unit, "service bundle activation").await?;
    }
    for unit in &plan.restart_units {
        wait_for_active_unit(unit).await?;
    }

    if plan.api_healthcheck
        && let Some(url) = config.api_healthcheck_url()
    {
        wait_for_healthcheck_url(url).await?;
    }

    Ok(())
}

async fn restart_systemd_unit(unit: &str, context: &str) -> Result<()> {
    let status = Command::new("systemctl").arg("restart").arg(unit).status().await?;
    if status.success() { Ok(()) } else { Err(Error::InvalidState(format!("failed to restart {} during {}", unit, context))) }
}

async fn wait_for_active_unit(unit: &str) -> Result<()> {
    for _ in 0..80 {
        let status = Command::new("systemctl").args(["is-active", "--quiet", unit]).status().await?;
        if status.success() {
            return Ok(());
        }
        sleep(Duration::from_millis(250)).await;
    }

    Err(Error::InvalidState(format!("{} did not become active after restart", unit)))
}

async fn wait_for_healthcheck_url(url: &str) -> Result<()> {
    let parsed = Url::parse(url).map_err(|err| Error::InvalidState(format!("invalid healthcheck url {url}: {err}")))?;
    if parsed.scheme() == "http" {
        return wait_for_http_plain(&parsed).await;
    }

    let client = build_https_healthcheck_client()?;
    wait_for_https_healthcheck(&client, url).await
}

async fn wait_for_http_plain(url: &Url) -> Result<()> {
    let host = url.host_str().ok_or_else(|| Error::InvalidState(format!("healthcheck url {} is missing a host", url)))?;
    let port = url.port_or_known_default().ok_or_else(|| Error::InvalidState(format!("healthcheck url {} is missing a port", url)))?;
    let mut path = url.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }

    let mut last_error = None;
    for _ in 0..80 {
        match TcpStream::connect((host, port)).await {
            Ok(mut stream) => {
                let host_header = if let Some(explicit_port) = url.port() { format!("{host}:{explicit_port}") } else { host.to_string() };
                let request = format!("GET {path} HTTP/1.1\r\nHost: {host_header}\r\nConnection: close\r\n\r\n");
                if let Err(err) = stream.write_all(request.as_bytes()).await {
                    last_error = Some(err.to_string());
                } else {
                    let mut buffer = [0u8; 256];
                    match stream.read(&mut buffer).await {
                        Ok(bytes_read) if bytes_read > 0 => {
                            let response = String::from_utf8_lossy(&buffer[..bytes_read]);
                            if let Some(status_line) = response.lines().next() {
                                let status_code = status_line.split_whitespace().nth(1).and_then(|value| value.parse::<u16>().ok());
                                if matches!(status_code, Some(200..=399)) {
                                    return Ok(());
                                }
                                last_error = Some(format!("status {}", status_code.unwrap_or_default()));
                            } else {
                                last_error = Some("missing HTTP status line".into());
                            }
                        }
                        Ok(_) => {
                            last_error = Some("empty HTTP response".into());
                        }
                        Err(err) => {
                            last_error = Some(err.to_string());
                        }
                    }
                }
            }
            Err(err) => {
                last_error = Some(err.to_string());
            }
        }
        sleep(Duration::from_millis(250)).await;
    }

    Err(Error::InvalidState(format!("healthcheck {} failed: {}", url, last_error.unwrap_or_else(|| "request timed out".into()))))
}

fn build_https_healthcheck_client() -> Result<reqwest::Client> {
    let roots = webpki_root_certs::TLS_SERVER_ROOT_CERTS.iter().map(|cert| reqwest::Certificate::from_der(cert.as_ref())).collect::<core::result::Result<Vec<_>, _>>()?;
    reqwest::Client::builder().timeout(Duration::from_secs(2)).tls_certs_only(roots).build().map_err(Into::into)
}

async fn wait_for_https_healthcheck(client: &reqwest::Client, url: &str) -> Result<()> {
    let mut last_error = None;
    for _ in 0..80 {
        match client.get(url).send().await {
            Ok(response) if response.status().is_success() => return Ok(()),
            Ok(response) => {
                last_error = Some(format!("status {}", response.status()));
            }
            Err(err) => {
                last_error = Some(err.to_string());
            }
        }
        sleep(Duration::from_millis(250)).await;
    }

    Err(Error::InvalidState(format!("healthcheck {} failed: {}", url, last_error.unwrap_or_else(|| StatusCode::REQUEST_TIMEOUT.to_string()))))
}

async fn ensure_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path).await?;
    Ok(())
}

async fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path).await {
        Ok(meta) => {
            if meta.is_dir() && !meta.file_type().is_symlink() {
                fs::remove_dir_all(path).await?;
            } else {
                fs::remove_file(path).await?;
            }
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(Error::Io(err)),
    }
}

async fn replace_active_path_with_symlink(active_path: &Path, target: &Path) -> Result<()> {
    let temp_link =
        active_path.parent().unwrap_or_else(|| Path::new("/")).join(format!(".{}.{}", active_path.file_name().and_then(|value| value.to_str()).unwrap_or("active"), Uuid::new_v4().simple()));
    remove_if_exists(&temp_link).await?;
    create_symlink(target, &temp_link)?;
    fs::rename(&temp_link, active_path).await?;
    Ok(())
}

#[cfg(unix)]
fn create_symlink(target: &Path, link: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link)?;
    Ok(())
}

#[cfg(not(unix))]
fn create_symlink(_target: &Path, _link: &Path) -> Result<()> {
    Err(Error::InvalidState("bundle activation requires unix symlink support".into()))
}

#[cfg(test)]
mod tests {
    use super::{
        FRONTEND_BUNDLE_KIND, SERVICE_BUNDLE_KIND, activate_service_bundle_release, apply_frontend_bundle, apply_service_bundle, build_service_bundle_plan, finalize_service_bundle_activation,
        is_frontend_bundle, is_service_bundle, rollback_service_bundle_release, validate_service_bundle_contents,
    };
    use crate::config::UpdaterConfig;
    use tempfile::tempdir;
    use uuid::Uuid;

    fn elf_bytes(body: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0x7f, b'E', b'L', b'F'];
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn bundle_kinds_match_expected_values() {
        assert!(is_frontend_bundle(Some(FRONTEND_BUNDLE_KIND)));
        assert!(is_service_bundle(Some(SERVICE_BUNDLE_KIND)));
        assert!(is_service_bundle(Some("service-bundle")));
        assert!(!is_frontend_bundle(Some("disk-image")));
        assert!(!is_service_bundle(Some("disk-image")));
        assert!(!is_service_bundle(None));
    }

    #[tokio::test]
    async fn frontend_bundle_activation_replaces_active_path_with_release_symlink() {
        let temp = tempdir().expect("temp dir");
        let releases_dir = temp.path().join("releases");
        let active_path = temp.path().join("active-frontend");
        let bundle_root = temp.path().join("bundle-root");
        tokio::fs::create_dir_all(&bundle_root).await.expect("create bundle root");
        tokio::fs::create_dir_all(&active_path).await.expect("create active frontend");
        tokio::fs::write(active_path.join("index.html"), b"old").await.expect("write old index");
        tokio::fs::write(bundle_root.join("index.html"), b"new").await.expect("write new index");
        tokio::fs::write(bundle_root.join("asset.txt"), b"asset").await.expect("write asset");

        let archive_path = temp.path().join("frontend-bundle.tar");
        let status = tokio::process::Command::new("tar").arg("-cf").arg(&archive_path).arg("-C").arg(&bundle_root).arg(".").status().await.expect("tar");
        assert!(status.success(), "failed to build test tar archive");

        let config = UpdaterConfig::new(temp.path().join("updater.sock"), temp.path().join("updater.log"))
            .with_frontend_paths(&releases_dir, &active_path)
            .with_frontend_service_unit("")
            .with_frontend_healthcheck_url(None::<String>);

        apply_frontend_bundle(&config, Uuid::nil(), &archive_path).await.expect("apply frontend bundle");

        let active_meta = tokio::fs::symlink_metadata(&active_path).await.expect("active path metadata");
        assert!(active_meta.file_type().is_symlink());

        let target = tokio::fs::read_link(&active_path).await.expect("read active symlink");
        assert_eq!(target, releases_dir.join(Uuid::nil().to_string()));
        let index = tokio::fs::read_to_string(target.join("index.html")).await.expect("read deployed index");
        assert_eq!(index, "new");
    }

    #[tokio::test]
    async fn frontend_bundle_activation_flattens_single_nested_root_and_cleans_legacy_backup() {
        let temp = tempdir().expect("temp dir");
        let releases_dir = temp.path().join("releases");
        let active_path = temp.path().join("active-frontend");
        let bundle_root = temp.path().join("bundle-root");
        let nested_root = bundle_root.join("dist");

        tokio::fs::create_dir_all(&nested_root).await.expect("create nested bundle root");
        tokio::fs::create_dir_all(&active_path).await.expect("create active frontend");
        tokio::fs::write(active_path.join("index.html"), b"old").await.expect("write old index");
        tokio::fs::write(nested_root.join("index.html"), b"new").await.expect("write new index");
        tokio::fs::write(nested_root.join("nested.txt"), b"nested").await.expect("write nested asset");

        let archive_path = temp.path().join("frontend-bundle-nested.tar");
        let status = tokio::process::Command::new("tar").arg("-cf").arg(&archive_path).arg("-C").arg(&bundle_root).arg(".").status().await.expect("tar");
        assert!(status.success(), "failed to build nested test tar archive");

        let config = UpdaterConfig::new(temp.path().join("updater.sock"), temp.path().join("updater.log"))
            .with_frontend_paths(&releases_dir, &active_path)
            .with_frontend_service_unit("")
            .with_frontend_healthcheck_url(None::<String>);

        apply_frontend_bundle(&config, Uuid::nil(), &archive_path).await.expect("apply frontend bundle");

        let target = tokio::fs::read_link(&active_path).await.expect("read active symlink");
        assert_eq!(target, releases_dir.join(Uuid::nil().to_string()));
        assert!(tokio::fs::metadata(target.join("index.html")).await.is_ok(), "flattened release should contain index.html at root");
        assert!(tokio::fs::metadata(target.join("nested.txt")).await.is_ok(), "flattened release should contain nested asset at root");

        let mut releases = tokio::fs::read_dir(&releases_dir).await.expect("read releases dir");
        let mut entries = Vec::new();
        while let Some(entry) = releases.next_entry().await.expect("read entry") {
            entries.push(entry.file_name().to_string_lossy().to_string());
        }
        assert_eq!(entries, vec![Uuid::nil().to_string()]);
    }

    #[tokio::test]
    async fn service_bundle_activation_replaces_regular_binary_with_release_symlink() {
        let temp = tempdir().expect("temp dir");
        let releases_dir = temp.path().join("service-releases");
        let bin_dir = temp.path().join("bin");
        let bundle_root = temp.path().join("bundle-root");
        tokio::fs::create_dir_all(&bin_dir).await.expect("create bin dir");
        tokio::fs::create_dir_all(&bundle_root).await.expect("create bundle dir");

        tokio::fs::write(bin_dir.join("helios-api-tools"), elf_bytes(b"old-tools")).await.expect("write old binary");
        tokio::fs::write(bundle_root.join("helios-api-tools"), elf_bytes(b"new-tools")).await.expect("write new binary");

        let archive_path = temp.path().join("service-bundle.tar");
        let status = tokio::process::Command::new("tar").arg("-cf").arg(&archive_path).arg("-C").arg(&bundle_root).arg(".").status().await.expect("tar");
        assert!(status.success(), "failed to build service tar archive");

        let config = UpdaterConfig::new(temp.path().join("updater.sock"), temp.path().join("updater.log"))
            .with_service_paths(&releases_dir, &bin_dir)
            .with_api_healthcheck_url(None::<String>)
            .with_updater_service_unit("");

        apply_service_bundle(&config, Uuid::nil(), &archive_path).await.expect("apply service bundle");

        let live_path = bin_dir.join("helios-api-tools");
        let live_meta = tokio::fs::symlink_metadata(&live_path).await.expect("live metadata");
        assert!(live_meta.file_type().is_symlink());
        let target = tokio::fs::read_link(&live_path).await.expect("read symlink");
        assert_eq!(target, releases_dir.join(Uuid::nil().to_string()).join("helios-api-tools"));
        let deployed = tokio::fs::read(target).await.expect("read deployed binary");
        assert_eq!(deployed, elf_bytes(b"new-tools"));

        let mut releases = tokio::fs::read_dir(&releases_dir).await.expect("read releases dir");
        let mut entries = Vec::new();
        while let Some(entry) = releases.next_entry().await.expect("read entry") {
            entries.push(entry.file_name().to_string_lossy().to_string());
        }
        assert_eq!(entries, vec![Uuid::nil().to_string()]);
    }

    #[tokio::test]
    async fn service_bundle_rolls_back_binary_symlink_activation() {
        let temp = tempdir().expect("temp dir");
        let releases_dir = temp.path().join("service-releases");
        let bin_dir = temp.path().join("bin");
        let release_dir = releases_dir.join(Uuid::nil().to_string());
        tokio::fs::create_dir_all(&bin_dir).await.expect("create bin dir");
        tokio::fs::create_dir_all(&release_dir).await.expect("create release dir");

        let live_path = bin_dir.join("helios-api-tools");
        tokio::fs::write(&live_path, elf_bytes(b"old-tools")).await.expect("write old live binary");
        tokio::fs::write(release_dir.join("helios-api-tools"), elf_bytes(b"new-tools")).await.expect("write new release binary");

        let config = UpdaterConfig::new(temp.path().join("updater.sock"), temp.path().join("updater.log"))
            .with_service_paths(&releases_dir, &bin_dir)
            .with_api_healthcheck_url(None::<String>)
            .with_updater_service_unit("");

        let specs = validate_service_bundle_contents(&release_dir).await.expect("validate service bundle contents");
        let plan = build_service_bundle_plan(&config, &release_dir, &specs);
        let activation = activate_service_bundle_release(&config, &plan).await.expect("activate service bundle");

        rollback_service_bundle_release(&activation).await.expect("rollback service bundle");

        let restored_meta = tokio::fs::symlink_metadata(&live_path).await.expect("restored metadata");
        assert!(!restored_meta.file_type().is_symlink());
        let restored = tokio::fs::read(&live_path).await.expect("read restored binary");
        assert_eq!(restored, elf_bytes(b"old-tools"));

        finalize_service_bundle_activation(&activation).await.expect("finalize activation");
    }
}
