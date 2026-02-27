use naga::valid::{Capabilities, ValidationFlags, Validator};
use std::{env, error::Error, fmt::Write, fs, path::Path};

fn main() {
    if let Err(err) = validate_shaders() {
        panic!("{err}");
    }
}

fn validate_shaders() -> Result<(), Box<dyn Error>> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;
    let shader_dir = Path::new(&manifest_dir).join("src/gpu/shaders");
    if !shader_dir.exists() {
        return Ok(());
    }
    println!("cargo:rerun-if-changed={}", shader_dir.display());
    let mut failures = Vec::new();
    collect_failures(&shader_dir, &mut failures)?;
    if failures.is_empty() {
        Ok(())
    } else {
        let mut message = String::from("WGSL shader validation failed:\n");
        for failure in failures {
            writeln!(message, "{failure}")?;
        }
        Err(message.into())
    }
}

fn collect_failures(dir: &Path, failures: &mut Vec<String>) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_failures(&path, failures)?;
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("wgsl") {
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        match validate_shader(&path) {
            Ok(()) => {}
            Err(err) => failures.push(err),
        }
    }
    Ok(())
}

fn validate_shader(path: &Path) -> Result<(), String> {
    let source = fs::read_to_string(path).map_err(|err| format!("Failed to read {}: {err}", path.display()))?;
    let module = match naga::front::wgsl::parse_str(&source) {
        Ok(module) => module,
        Err(err) => {
            let path_string = path.display().to_string();
            let pretty = err.emit_to_string_with_path(&source, path_string.as_str());
            return Err(format!("{}:\n{}", path.display(), pretty.trim_end()));
        }
    };
    let mut validator = Validator::new(ValidationFlags::all(), Capabilities::all());
    if let Err(err) = validator.validate(&module) {
        let path_string = path.display().to_string();
        let pretty = err.emit_to_string_with_path(&source, path_string.as_str());
        return Err(format!("{}:\n{}", path.display(), pretty.trim_end()));
    }
    Ok(())
}
