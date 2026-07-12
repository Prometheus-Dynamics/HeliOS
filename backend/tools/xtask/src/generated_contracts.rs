use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const REQUIRED_FRONTEND_GENERATED_PATHS: &[&str] = &[
    "frontend/src/generated/types.ts",
    "frontend/src/generated/codecFamilies.ts",
    "frontend/src/generated/runtimeContracts.ts",
    "frontend/src/generated/http/openapi.json",
    "frontend/src/generated/http/client/index.ts",
    "frontend/src/generated/http/client/core/OpenAPI.ts",
    "frontend/src/generated/ws/asyncapi.json",
];

const TRACKED_GENERATED_PATHS: &[&str] = &["backend/src/helios/engine/src/contracts/generated_runtime_contracts.rs"];
const RUNTIME_CONTRACTS_SPEC_PATH: &str = "tools/api-codegen/runtime-contracts.toml";
const CODEC_FAMILIES_SPEC_PATH: &str = "tools/api-codegen/codec-families.toml";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct ContractMetadata {
    owner: String,
    boundary: String,
    source_of_truth: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct ReservedPipelineIdsContract {
    #[serde(flatten)]
    metadata: ContractMetadata,
    raw_pipeline_uuid: String,
    calibration_mode_pipeline_uuid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct LocalizationExternalSourceIdsContract {
    #[serde(flatten)]
    metadata: ContractMetadata,
    device_imu_external_source_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct RuntimeContractsSpec {
    schema_version: u32,
    reserved_pipeline_ids: ReservedPipelineIdsContract,
    localization_external_source_ids: LocalizationExternalSourceIdsContract,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct CodecFamiliesSpec {
    schema_version: u32,
    spec: ContractMetadata,
    encoder_families: Vec<CodecFamilySpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct CodecFamilySpec {
    id: String,
    rust_variant: String,
    settings_kind: String,
    selector_id: String,
    selector_aliases: Vec<String>,
    runtime_implementation_aliases: Vec<String>,
    runtime_name_aliases: Vec<String>,
    output_fourcc_aliases: Vec<String>,
    recording_codec: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CodecFamilyTsSpec<'a> {
    id: &'a str,
    rust_variant: &'a str,
    settings_kind: &'a str,
    selector_id: &'a str,
    selector_aliases: &'a [String],
    runtime_implementation_aliases: &'a [String],
    runtime_name_aliases: &'a [String],
    output_fourcc_aliases: &'a [String],
    recording_codec: Option<&'a str>,
}

pub fn generate(repo_root: &Path) -> Result<()> {
    let backend_dir = repo_root.join("backend");
    let client_template_dir = repo_root.join("tools").join("api-client");
    let overrides_dir = repo_root.join("tools").join("api-codegen").join("overrides");
    let ts_bindings_root = repo_root.join("frontend").join("src").join("generated");
    let http_bindings_dir = ts_bindings_root.join("http");
    let ws_bindings_dir = ts_bindings_root.join("ws");
    let http_client_dir = http_bindings_dir.join("client");
    let temp_root = repo_root.join(".tmp").join("api-codegen");
    let cargo_target_dir = repo_root.join(".tmp").join("api-codegen-target");
    let spec_dir = unique_temp_dir(&temp_root, "helios-spec")?;
    let tmp_dir = spec_dir.join("tmp");
    let http_spec = spec_dir.join("http.json");
    let ws_spec = spec_dir.join("ws.json");

    fs::create_dir_all(&tmp_dir).with_context(|| format!("failed to create {}", tmp_dir.display()))?;
    fs::create_dir_all(&cargo_target_dir).with_context(|| format!("failed to create {}", cargo_target_dir.display()))?;

    log("spec", "Generating OpenAPI + AsyncAPI descriptions via helios-api CLI");
    run_command(
        "cargo",
        ["run", "-p", "helios-api", "--", "apispec", "--http", http_spec.to_str().unwrap(), "--ws", ws_spec.to_str().unwrap()],
        &backend_dir,
        &[("TMPDIR", tmp_dir.as_os_str()), ("CARGO_TARGET_DIR", cargo_target_dir.as_os_str())],
    )?;

    log("deps", "Installing JS dependencies with bun");
    if let Err(error) = run_command("bun", ["install"], &client_template_dir, &[]) {
        log("deps", &format!("bun install failed ({error}); retrying after removing node_modules"));
        remove_path(&client_template_dir.join("node_modules"))?;
        run_command("bun", ["install"], &client_template_dir, &[])?;
    }

    remove_path(&ts_bindings_root)?;
    fs::create_dir_all(&http_bindings_dir).with_context(|| format!("failed to create {}", http_bindings_dir.display()))?;
    fs::create_dir_all(&ws_bindings_dir).with_context(|| format!("failed to create {}", ws_bindings_dir.display()))?;

    log("types", "Generating shared TypeScript types");
    run_command("bun", ["x", "openapi-typescript", http_spec.to_str().unwrap(), "-o", ts_bindings_root.join("types.ts").to_str().unwrap()], &client_template_dir, &[])?;

    log("http-client", "Generating HTTP client bindings");
    remove_path(&http_client_dir)?;
    run_command(
        "bun",
        ["x", "openapi-typescript-codegen", "--input", http_spec.to_str().unwrap(), "--output", http_client_dir.to_str().unwrap(), "--useOptions", "--useUnionTypes"],
        &client_template_dir,
        &[],
    )?;
    if overrides_dir.exists() {
        copy_dir_recursive(&overrides_dir, &http_client_dir)?;
        log("http-client", "Applied custom overrides");
    }

    fs::copy(&http_spec, http_bindings_dir.join("openapi.json")).with_context(|| format!("failed to copy {}", http_spec.display()))?;
    fs::copy(&ws_spec, ws_bindings_dir.join("asyncapi.json")).with_context(|| format!("failed to copy {}", ws_spec.display()))?;

    log("codec-families", "Generating shared codec family bindings");
    generate_codec_families(repo_root)?;
    log("runtime-contracts", "Generating shared runtime contract bindings");
    generate_runtime_contracts(repo_root)?;
    log("done", &format!("Artifacts written to {}", ts_bindings_root.strip_prefix(repo_root).unwrap_or(&ts_bindings_root).display()));

    remove_path(&spec_dir)?;
    Ok(())
}

pub fn validate(repo_root: &Path) -> Result<()> {
    let ts_bindings_root = repo_root.join("frontend").join("src").join("generated");
    log("check", "Removing ignored frontend bindings to simulate a fresh clone");
    remove_path(&ts_bindings_root)?;

    log("check", "Regenerating API bindings");
    generate(repo_root)?;

    log("check", "Verifying frontend generated binding surface");
    for relative_path in REQUIRED_FRONTEND_GENERATED_PATHS {
        let candidate = repo_root.join(relative_path);
        if !candidate.is_file() {
            bail!("missing generated frontend binding {relative_path}");
        }
    }

    log("check", "Verifying tracked generated bindings are committed");
    run_git_diff(repo_root, TRACKED_GENERATED_PATHS).context("generated bindings are out of date. Re-run frontend codegen and commit the changes.")?;
    log("check", "Generated bindings are in sync");
    Ok(())
}

fn generate_runtime_contracts(repo_root: &Path) -> Result<()> {
    let spec_path = repo_root.join(RUNTIME_CONTRACTS_SPEC_PATH);
    let spec = load_runtime_contracts_spec(&spec_path)?;

    let rust_output_path = repo_root.join("backend").join("src").join("helios-engine").join("src").join("contracts").join("generated_runtime_contracts.rs");
    let ts_output_path = repo_root.join("frontend").join("src").join("generated").join("runtimeContracts.ts");
    write_generated_file(&rust_output_path, &render_runtime_contracts_rust(&spec))?;
    write_generated_file(&ts_output_path, &render_runtime_contracts_ts(&spec))?;
    Ok(())
}

fn generate_codec_families(repo_root: &Path) -> Result<()> {
    let spec_path = repo_root.join(CODEC_FAMILIES_SPEC_PATH);
    let spec = load_codec_families_spec(&spec_path)?;

    let ts_output_path = repo_root.join("frontend").join("src").join("generated").join("codecFamilies.ts");
    write_generated_file(&ts_output_path, &render_codec_families_ts(&spec)?)?;
    Ok(())
}

fn load_runtime_contracts_spec(spec_path: &Path) -> Result<RuntimeContractsSpec> {
    let raw = fs::read_to_string(spec_path).with_context(|| format!("failed to read {}", spec_path.display()))?;
    let spec: RuntimeContractsSpec = toml::from_str(&raw).with_context(|| format!("failed to parse {}", spec_path.display()))?;
    validate_runtime_contracts_spec(spec, spec_path)
}

fn load_codec_families_spec(spec_path: &Path) -> Result<CodecFamiliesSpec> {
    let raw = fs::read_to_string(spec_path).with_context(|| format!("failed to read {}", spec_path.display()))?;
    let spec: CodecFamiliesSpec = toml::from_str(&raw).with_context(|| format!("failed to parse {}", spec_path.display()))?;
    validate_codec_families_spec(spec, spec_path)
}

fn validate_runtime_contracts_spec(spec: RuntimeContractsSpec, spec_path: &Path) -> Result<RuntimeContractsSpec> {
    if spec.schema_version != 1 {
        bail!("{} must declare schema_version = 1", spec_path.display());
    }

    validate_contract_metadata(&spec.reserved_pipeline_ids.metadata, "reserved_pipeline_ids", spec_path)?;
    validate_non_empty(&spec.reserved_pipeline_ids.raw_pipeline_uuid, "raw_pipeline_uuid", "reserved_pipeline_ids", spec_path)?;
    validate_non_empty(&spec.reserved_pipeline_ids.calibration_mode_pipeline_uuid, "calibration_mode_pipeline_uuid", "reserved_pipeline_ids", spec_path)?;

    validate_contract_metadata(&spec.localization_external_source_ids.metadata, "localization_external_source_ids", spec_path)?;
    validate_non_empty(&spec.localization_external_source_ids.device_imu_external_source_id, "device_imu_external_source_id", "localization_external_source_ids", spec_path)?;

    Ok(spec)
}

fn validate_codec_families_spec(spec: CodecFamiliesSpec, spec_path: &Path) -> Result<CodecFamiliesSpec> {
    if spec.schema_version != 1 {
        bail!("{} must declare schema_version = 1", spec_path.display());
    }

    validate_contract_metadata(&spec.spec, "spec", spec_path)?;
    if spec.encoder_families.is_empty() {
        bail!("{} must define at least one encoder_families entry", spec_path.display());
    }

    for family in &spec.encoder_families {
        let context = format!("encoder_families[{}]", family.id);
        validate_non_empty(&family.id, "id", &context, spec_path)?;
        validate_non_empty(&family.rust_variant, "rust_variant", &context, spec_path)?;
        validate_non_empty(&family.settings_kind, "settings_kind", &context, spec_path)?;
        validate_non_empty(&family.selector_id, "selector_id", &context, spec_path)?;
    }

    Ok(spec)
}

fn validate_contract_metadata(metadata: &ContractMetadata, context: &str, spec_path: &Path) -> Result<()> {
    validate_non_empty(&metadata.owner, "owner", context, spec_path)?;
    validate_non_empty(&metadata.boundary, "boundary", context, spec_path)?;
    validate_non_empty(&metadata.source_of_truth, "source_of_truth", context, spec_path)?;
    Ok(())
}

fn validate_non_empty(value: &str, field_name: &str, context: &str, spec_path: &Path) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{} {} is missing required field {}", spec_path.display(), context, field_name);
    }
    Ok(())
}

fn render_runtime_contracts_rust(spec: &RuntimeContractsSpec) -> String {
    format!(
        "// Generated by backend/tools/xtask from {RUNTIME_CONTRACTS_SPEC_PATH}. Do not edit by hand.\n// reserved_pipeline_ids owner: {} | boundary: {} | source_of_truth: {}\n// localization_external_source_ids owner: {} | boundary: {} | source_of_truth: {}\n\npub mod stream_ids {{\n    use uuid::Uuid;\n\n    pub const CALIBRATION_MODE_PIPELINE_UUID: Uuid = uuid::uuid!({:?});\n    pub const RAW_PIPELINE_UUID: Uuid = uuid::uuid!({:?});\n}}\n\npub mod localization {{\n    pub const DEVICE_IMU_EXTERNAL_SOURCE_ID: &str = {:?};\n}}\n",
        spec.reserved_pipeline_ids.metadata.owner,
        spec.reserved_pipeline_ids.metadata.boundary,
        spec.reserved_pipeline_ids.metadata.source_of_truth,
        spec.localization_external_source_ids.metadata.owner,
        spec.localization_external_source_ids.metadata.boundary,
        spec.localization_external_source_ids.metadata.source_of_truth,
        spec.reserved_pipeline_ids.calibration_mode_pipeline_uuid,
        spec.reserved_pipeline_ids.raw_pipeline_uuid,
        spec.localization_external_source_ids.device_imu_external_source_id,
    )
}

fn render_runtime_contracts_ts(spec: &RuntimeContractsSpec) -> String {
    let device_imu_external_source_id = &spec.localization_external_source_ids.device_imu_external_source_id;

    format!(
        "/* Generated by backend/tools/xtask from {RUNTIME_CONTRACTS_SPEC_PATH}. Do not edit by hand. */\n\
/* reserved_pipeline_ids owner: {} | boundary: {} | source_of_truth: {} */\n\
/* localization_external_source_ids owner: {} | boundary: {} | source_of_truth: {} */\n\n\
export const CALIBRATION_MODE_PIPELINE_UUID = {:?};\n\
export const RAW_PIPELINE_UUID = {:?};\n\
export const DEVICE_IMU_EXTERNAL_SOURCE_ID = {:?};\n\n\
export const STREAM_PIPELINE_UUIDS = {{\n  RAW: RAW_PIPELINE_UUID,\n  CALIBRATION_MODE: CALIBRATION_MODE_PIPELINE_UUID\n}} as const;\n\n\
export const LOCALIZATION_EXTERNAL_SOURCE_IDS = {{\n  DEVICE_IMU: DEVICE_IMU_EXTERNAL_SOURCE_ID\n}} as const;\n\n\
export const LOCALIZATION_EXTERNAL_STREAM_IDS = {{\n  DEVICE_IMU: {:?}\n}} as const;\n",
        spec.reserved_pipeline_ids.metadata.owner,
        spec.reserved_pipeline_ids.metadata.boundary,
        spec.reserved_pipeline_ids.metadata.source_of_truth,
        spec.localization_external_source_ids.metadata.owner,
        spec.localization_external_source_ids.metadata.boundary,
        spec.localization_external_source_ids.metadata.source_of_truth,
        spec.reserved_pipeline_ids.calibration_mode_pipeline_uuid,
        spec.reserved_pipeline_ids.raw_pipeline_uuid,
        device_imu_external_source_id,
        format!("external:{device_imu_external_source_id}")
    )
}

fn render_codec_families_ts(spec: &CodecFamiliesSpec) -> Result<String> {
    let id_entries = spec.encoder_families.iter().map(|family| format!("  {}: {:?}", constant_case(&family.id), family.id)).collect::<Vec<_>>().join(",\n");
    let selector_entries = spec.encoder_families.iter().map(|family| format!("  {}: {:?}", constant_case(&family.id), family.selector_id)).collect::<Vec<_>>().join(",\n");
    let families = spec
        .encoder_families
        .iter()
        .map(|family| CodecFamilyTsSpec {
            id: &family.id,
            rust_variant: &family.rust_variant,
            settings_kind: &family.settings_kind,
            selector_id: &family.selector_id,
            selector_aliases: &family.selector_aliases,
            runtime_implementation_aliases: &family.runtime_implementation_aliases,
            runtime_name_aliases: &family.runtime_name_aliases,
            output_fourcc_aliases: &family.output_fourcc_aliases,
            recording_codec: family.recording_codec.as_deref(),
        })
        .collect::<Vec<_>>();
    let families_json = serde_json::to_string_pretty(&families).context("failed to serialize codec family spec")?;

    Ok(format!(
        "/* Generated by backend/tools/xtask from {CODEC_FAMILIES_SPEC_PATH}. Do not edit by hand. */\n\
/* owner: {} | boundary: {} | source_of_truth: {} */\n\n\
export const STREAM_ENCODER_FAMILY_IDS = {{\n{id_entries}\n}} as const;\n\n\
export const STREAM_ENCODER_SELECTOR_IDS = {{\n{selector_entries}\n}} as const;\n\n\
export const STREAM_ENCODER_FAMILIES = {families_json} as const;\n\n\
export type StreamEncoderFamily = (typeof STREAM_ENCODER_FAMILIES)[number];\n\
export type StreamEncoderFamilyId = StreamEncoderFamily['id'];\n\
export type StreamEncoderSelectionId = StreamEncoderFamily['selectorId'];\n\
export type StreamRecordingCodecId = Exclude<StreamEncoderFamily['recordingCodec'], null>;\n",
        spec.spec.owner, spec.spec.boundary, spec.spec.source_of_truth,
    ))
}

fn upper_snake(value: &str) -> String {
    let mut out = String::new();
    let mut prev_was_alnum = false;
    let mut prev_was_lower_or_digit = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            let is_upper = ch.is_ascii_uppercase();
            if !out.is_empty() && (is_upper && prev_was_lower_or_digit) && prev_was_alnum && !out.ends_with('_') {
                out.push('_');
            }
            out.push(ch.to_ascii_uppercase());
            prev_was_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
            prev_was_alnum = true;
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
            prev_was_lower_or_digit = false;
            prev_was_alnum = false;
        }
    }
    out.trim_matches('_').to_string()
}

fn constant_case(value: &str) -> String {
    upper_snake(value)
}

fn unique_temp_dir(root: &Path, prefix: &str) -> Result<PathBuf> {
    fs::create_dir_all(root).with_context(|| format!("failed to create {}", root.display()))?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let path = root.join(format!("{prefix}-{stamp}"));
    fs::create_dir_all(&path).with_context(|| format!("failed to create {}", path.display()))?;
    Ok(path)
}

fn run_command<I, S>(cmd: &str, args: I, cwd: &Path, envs: &[(&str, &std::ffi::OsStr)]) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args_vec = args.into_iter().map(|arg| arg.as_ref().to_string()).collect::<Vec<_>>();
    let mut command = Command::new(cmd);
    if cmd == "cargo"
        && let Some(config_path) = std::env::var_os("HELIOS_CARGO_CONFIG")
    {
        let config_path = PathBuf::from(config_path);
        let config_path = if config_path.is_absolute() { config_path } else { std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(config_path) };
        command.arg("--config").arg(config_path);
    }
    command.args(&args_vec).current_dir(cwd).env_remove("RUSTC_WRAPPER");
    for (key, value) in envs {
        command.env(key, value);
    }
    let status = command.status().with_context(|| format!("failed to launch {cmd}"))?;
    if !status.success() {
        bail!("{cmd} {} exited with status {status}", args_vec.join(" "));
    }
    Ok(())
}

fn run_git_diff(repo_root: &Path, paths: &[&str]) -> Result<()> {
    let status = Command::new("git").args(["diff", "--exit-code", "--"]).args(paths).current_dir(repo_root).status().context("failed to launch git diff")?;
    if !status.success() {
        bail!("git diff detected tracked generated changes");
    }
    Ok(())
}

fn copy_dir_recursive(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to).with_context(|| format!("failed to create {}", to.display()))?;
    for entry in fs::read_dir(from).with_context(|| format!("failed to read {}", from.display()))? {
        let entry = entry.with_context(|| format!("failed to read entry in {}", from.display()))?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            copy_dir_recursive(&source, &target)?;
        } else if source.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::copy(&source, &target).with_context(|| format!("failed to copy {} to {}", source.display(), target.display()))?;
        }
    }
    Ok(())
}

fn write_generated_file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, contents).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn remove_path(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        fs::remove_dir_all(path).with_context(|| format!("failed to remove {}", path.display()))?;
    } else {
        fs::remove_file(path).with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}

fn log(step: &str, message: &str) {
    println!("[api-codegen:{step}] {message}");
}

#[cfg(test)]
mod tests {
    use super::{
        CODEC_FAMILIES_SPEC_PATH, ContractMetadata, LocalizationExternalSourceIdsContract, RUNTIME_CONTRACTS_SPEC_PATH, ReservedPipelineIdsContract, RuntimeContractsSpec,
        validate_codec_families_spec, validate_runtime_contracts_spec,
    };

    use std::path::Path;

    #[test]
    fn runtime_contracts_require_owner_metadata() {
        let spec = RuntimeContractsSpec {
            schema_version: 1,
            reserved_pipeline_ids: ReservedPipelineIdsContract {
                metadata: ContractMetadata { owner: String::new(), boundary: "helios-engine <-> frontend".into(), source_of_truth: "backend/src/helios/engine/src/contracts.rs".into() },
                raw_pipeline_uuid: "00000000-0000-0000-0000-0000000000aa".into(),
                calibration_mode_pipeline_uuid: "00000000-0000-0000-0000-00000000c411".into(),
            },
            localization_external_source_ids: LocalizationExternalSourceIdsContract {
                metadata: ContractMetadata { owner: "HeliOS".into(), boundary: "helios-api <-> frontend".into(), source_of_truth: "backend/src/helios/api/src/http/localization/config.rs".into() },
                device_imu_external_source_id: "imu".into(),
            },
        };

        let err = validate_runtime_contracts_spec(spec, Path::new(RUNTIME_CONTRACTS_SPEC_PATH)).expect_err("missing owner should fail");
        assert!(err.to_string().contains("reserved_pipeline_ids is missing required field owner"));
    }

    #[test]
    fn codec_families_require_owner_metadata() {
        let raw = r#"
schema_version = 1

[spec]
owner = ""
boundary = "helios-api/engine <-> frontend"
source_of_truth = "tools/api-codegen/codec-families.toml"

[[encoder_families]]
id = "turbojpeg"
rust_variant = "Turbojpeg"
settings_kind = "turbojpeg"
selector_id = "turbojpeg"
selector_aliases = ["turbojpeg"]
runtime_implementation_aliases = ["turbojpeg"]
runtime_name_aliases = []
output_fourcc_aliases = ["MJPG", "JPEG"]
"#;

        let spec: super::CodecFamiliesSpec = toml::from_str(raw).expect("parse spec");
        let err = validate_codec_families_spec(spec, Path::new(CODEC_FAMILIES_SPEC_PATH)).expect_err("missing owner should fail");
        assert!(err.to_string().contains("spec is missing required field owner"));
    }

    #[test]
    fn runtime_contracts_render_comments_with_owner_metadata() {
        let spec = RuntimeContractsSpec {
            schema_version: 1,
            reserved_pipeline_ids: ReservedPipelineIdsContract {
                metadata: ContractMetadata { owner: "HeliOS".into(), boundary: "helios-engine <-> frontend".into(), source_of_truth: "backend/src/helios/engine/src/contracts.rs".into() },
                raw_pipeline_uuid: "00000000-0000-0000-0000-0000000000aa".into(),
                calibration_mode_pipeline_uuid: "00000000-0000-0000-0000-00000000c411".into(),
            },
            localization_external_source_ids: LocalizationExternalSourceIdsContract {
                metadata: ContractMetadata { owner: "HeliOS".into(), boundary: "helios-api <-> frontend".into(), source_of_truth: "backend/src/helios/api/src/http/localization/config.rs".into() },
                device_imu_external_source_id: "imu".into(),
            },
        };

        let rendered = super::render_runtime_contracts_ts(&spec);
        assert!(rendered.contains("reserved_pipeline_ids owner: HeliOS"));
        assert!(rendered.contains("LOCALIZATION_EXTERNAL_STREAM_IDS"));
    }
}
