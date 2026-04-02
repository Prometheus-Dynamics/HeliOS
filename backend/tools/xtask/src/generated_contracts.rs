use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

const REQUIRED_FRONTEND_GENERATED_PATHS: &[&str] = &[
    "frontend/src/lib/ts-bindings/types.ts",
    "frontend/src/lib/ts-bindings/codecFamilies.ts",
    "frontend/src/lib/ts-bindings/runtimeContracts.ts",
    "frontend/src/lib/ts-bindings/http/openapi.json",
    "frontend/src/lib/ts-bindings/http/client/index.ts",
    "frontend/src/lib/ts-bindings/http/client/core/OpenAPI.ts",
    "frontend/src/lib/ts-bindings/ws/asyncapi.json",
];

const TRACKED_GENERATED_PATHS: &[&str] = &["backend/src/helios-engine/src/ipc/types/generated_codec_families.rs", "backend/src/helios-engine/src/contracts/generated_runtime_contracts.rs"];

#[derive(Debug, Deserialize)]
struct RuntimeContractsSpec {
    #[serde(rename = "streamIds")]
    stream_ids: BTreeMap<String, String>,
    localization: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct CodecFamiliesSpec {
    #[serde(rename = "encoderFamilies")]
    encoder_families: Vec<CodecFamilySpec>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct CodecFamilySpec {
    id: String,
    #[serde(rename = "rustVariant")]
    rust_variant: String,
    #[serde(rename = "settingsKind")]
    settings_kind: String,
    #[serde(rename = "selectorId")]
    selector_id: String,
    #[serde(rename = "selectorAliases")]
    selector_aliases: Vec<String>,
    #[serde(rename = "runtimeImplementationAliases")]
    runtime_implementation_aliases: Vec<String>,
    #[serde(rename = "runtimeNameAliases")]
    runtime_name_aliases: Vec<String>,
    #[serde(rename = "outputFourccAliases")]
    output_fourcc_aliases: Vec<String>,
    #[serde(rename = "recordingCodec")]
    recording_codec: Option<String>,
}

pub fn generate(repo_root: &Path) -> Result<()> {
    let backend_dir = repo_root.join("backend");
    let client_template_dir = repo_root.join("tools").join("api-client");
    let overrides_dir = repo_root.join("tools").join("api-codegen").join("overrides");
    let ts_bindings_root = repo_root.join("frontend").join("src").join("lib").join("ts-bindings");
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
    let ts_bindings_root = repo_root.join("frontend").join("src").join("lib").join("ts-bindings");
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
    let spec_path = repo_root.join("tools").join("api-codegen").join("runtime-contracts.json");
    let spec: RuntimeContractsSpec = serde_json::from_str(&fs::read_to_string(&spec_path).with_context(|| format!("failed to read {}", spec_path.display()))?)
        .with_context(|| format!("failed to parse {}", spec_path.display()))?;

    let rust_output_path = repo_root.join("backend").join("src").join("helios-engine").join("src").join("contracts").join("generated_runtime_contracts.rs");
    let ts_output_path = repo_root.join("frontend").join("src").join("lib").join("ts-bindings").join("runtimeContracts.ts");
    write_generated_file(&rust_output_path, &render_runtime_contracts_rust(&spec))?;
    write_generated_file(&ts_output_path, &render_runtime_contracts_ts(&spec))?;
    Ok(())
}

fn generate_codec_families(repo_root: &Path) -> Result<()> {
    let spec_path = repo_root.join("tools").join("api-codegen").join("codec-families.json");
    let spec: CodecFamiliesSpec = serde_json::from_str(&fs::read_to_string(&spec_path).with_context(|| format!("failed to read {}", spec_path.display()))?)
        .with_context(|| format!("failed to parse {}", spec_path.display()))?;

    let rust_output_path = repo_root.join("backend").join("src").join("helios-engine").join("src").join("ipc").join("types").join("generated_codec_families.rs");
    let ts_output_path = repo_root.join("frontend").join("src").join("lib").join("ts-bindings").join("codecFamilies.ts");
    write_generated_file(&rust_output_path, &render_codec_families_rust(&spec))?;
    write_generated_file(&ts_output_path, &render_codec_families_ts(&spec)?)?;
    Ok(())
}

fn render_runtime_contracts_rust(spec: &RuntimeContractsSpec) -> String {
    let stream_entries = spec.stream_ids.iter().map(|(key, value)| format!("    pub const {}: Uuid = uuid::uuid!({:?});", upper_snake(key), value)).collect::<Vec<_>>().join("\n");
    let localization_entries = spec.localization.iter().map(|(key, value)| format!("    pub const {}: &str = {:?};", upper_snake(key), value)).collect::<Vec<_>>().join("\n");

    format!("// Generated by backend/tools/xtask. Do not edit by hand.\n\npub mod stream_ids {{\n    use uuid::Uuid;\n\n{stream_entries}\n}}\n\npub mod localization {{\n{localization_entries}\n}}\n")
}

fn render_runtime_contracts_ts(spec: &RuntimeContractsSpec) -> String {
    let stream_entries = spec.stream_ids.iter().map(|(key, value)| format!("export const {} = {:?};", upper_snake(key), value)).collect::<Vec<_>>().join("\n");
    let localization_entries = spec.localization.iter().map(|(key, value)| format!("export const {} = {:?};", upper_snake(key), value)).collect::<Vec<_>>().join("\n");
    let device_imu_external_source_id = spec.localization.get("deviceImuExternalSourceId").cloned().unwrap_or_default();

    format!(
        "/* Generated by backend/tools/xtask. Do not edit by hand. */\n\n{stream_entries}\n{localization_entries}\n\nexport const STREAM_PIPELINE_UUIDS = {{\n  RAW: RAW_PIPELINE_UUID,\n  LEGACY_RAW: LEGACY_RAW_PIPELINE_UUID,\n  CALIBRATION_MODE: CALIBRATION_MODE_PIPELINE_UUID\n}} as const;\n\nexport const LOCALIZATION_EXTERNAL_SOURCE_IDS = {{\n  DEVICE_IMU: DEVICE_IMU_EXTERNAL_SOURCE_ID\n}} as const;\n\nexport const LOCALIZATION_EXTERNAL_STREAM_IDS = {{\n  DEVICE_IMU: {:?}\n}} as const;\n",
        format!("external:{device_imu_external_source_id}")
    )
}

fn render_codec_families_rust(spec: &CodecFamiliesSpec) -> String {
    let selector_consts = spec
        .encoder_families
        .iter()
        .map(|family| format!("pub(super) const {}: &str = {:?};", constant_case(&format!("encoder_selector_id_{}", family.id)), family.selector_id))
        .collect::<Vec<_>>()
        .join("\n");

    let entries = spec
        .encoder_families
        .iter()
        .map(|family| {
            format!(
                "    GeneratedEncoderFamilySpec {{\n        variant: GeneratedEncoderFamilyVariant::{},\n        selector_id: {},\n        selector_aliases: {},\n        runtime_implementation_aliases: {},\n        runtime_name_aliases: {},\n        output_fourcc_aliases: {},\n        recording_codec: {},\n    }}",
                family.rust_variant,
                constant_case(&format!("encoder_selector_id_{}", family.id)),
                render_rust_array(&family.selector_aliases),
                render_rust_array(&family.runtime_implementation_aliases),
                render_rust_array(&family.runtime_name_aliases),
                render_rust_array(&family.output_fourcc_aliases),
                family.recording_codec.as_ref().map(|value| format!("Some({value:?})")).unwrap_or_else(|| "None".to_string()),
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");

    format!(
        "// Generated by backend/tools/xtask. Do not edit by hand.\n\n#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]\npub(super) enum GeneratedEncoderFamilyVariant {{\n{}\n}}\n\n#[derive(Debug, Clone, Copy)]\npub(super) struct GeneratedEncoderFamilySpec {{\n    pub variant: GeneratedEncoderFamilyVariant,\n    pub selector_id: &'static str,\n    pub selector_aliases: &'static [&'static str],\n    pub runtime_implementation_aliases: &'static [&'static str],\n    pub runtime_name_aliases: &'static [&'static str],\n    pub output_fourcc_aliases: &'static [&'static str],\n    pub recording_codec: Option<&'static str>,\n}}\n\n{selector_consts}\n\npub(super) const GENERATED_ENCODER_FAMILY_SPECS: &[GeneratedEncoderFamilySpec] = &[\n{entries}\n];\n",
        spec.encoder_families.iter().map(|family| format!("    {},", family.rust_variant)).collect::<Vec<_>>().join("\n"),
    )
}

fn render_codec_families_ts(spec: &CodecFamiliesSpec) -> Result<String> {
    let id_entries = spec.encoder_families.iter().map(|family| format!("  {}: {:?}", constant_case(&family.id), family.id)).collect::<Vec<_>>().join(",\n");
    let selector_entries = spec.encoder_families.iter().map(|family| format!("  {}: {:?}", constant_case(&family.id), family.selector_id)).collect::<Vec<_>>().join(",\n");
    let families_json = serde_json::to_string_pretty(&spec.encoder_families).context("failed to serialize codec family spec")?;

    Ok(format!(
        "/* Generated by backend/tools/xtask. Do not edit by hand. */\n\nexport const STREAM_ENCODER_FAMILY_IDS = {{\n{id_entries}\n}} as const;\n\nexport const STREAM_ENCODER_SELECTOR_IDS = {{\n{selector_entries}\n}} as const;\n\nexport const STREAM_ENCODER_FAMILIES = {families_json} as const;\n\nexport type StreamEncoderFamily = (typeof STREAM_ENCODER_FAMILIES)[number];\nexport type StreamEncoderFamilyId = StreamEncoderFamily['id'];\nexport type StreamEncoderSelectionId = StreamEncoderFamily['selectorId'];\nexport type StreamRecordingCodecId = Exclude<StreamEncoderFamily['recordingCodec'], null>;\n"
    ))
}

fn render_rust_array(values: &[String]) -> String {
    if values.is_empty() {
        return "&[]".to_string();
    }
    format!("&[{}]", values.iter().map(|value| format!("{value:?}")).collect::<Vec<_>>().join(", "))
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
