#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{fs, path::Path};

pub(super) fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else if ty.is_file() {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

pub(super) fn patch_cargo_toml(path: &Path, daedalus_crate: &Path) -> std::io::Result<()> {
    let content = fs::read_to_string(path)?;
    let mut out = String::with_capacity(content.len());
    for line in content.lines() {
        if line.contains("daedalus") && line.contains("path") && line.contains('{') {
            let mut updated = line.to_string();
            if let Some(start) = updated.find("path = \"") {
                let rest = &updated[start + 8..];
                if let Some(end) = rest.find('"') {
                    let mut patched = String::new();
                    patched.push_str(&updated[..start + 8]);
                    patched.push_str(&daedalus_crate.display().to_string());
                    patched.push_str(&rest[end..]);
                    updated = patched;
                }
            }
            out.push_str(&updated);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    fs::write(path, out)
}

pub(super) fn patch_cpp_build(dest: &Path) -> std::io::Result<()> {
    let build_path = dest.join("build.sh");
    if !build_path.exists() {
        return Ok(());
    }
    let script = [
        "#!/usr/bin/env bash",
        "set -euo pipefail",
        "",
        "OUT_DIR=\"${1:-/tmp/example_cpp}\"",
        "mkdir -p \"$OUT_DIR\"",
        "",
        "ROOT=\"$(cd \"$(dirname \"${BASH_SOURCE[0]}\")\" && pwd)\"",
        "",
        "SRC=\"$ROOT/nodes.cpp\"",
        "HDR=\"$ROOT/sdk/daedalus_c_cpp.h\"",
        "SHADERS_DIR=\"$ROOT/shaders\"",
        "",
        "OS=\"$(uname -s | tr '[:upper:]' '[:lower:]')\"",
        "LIB_EXT=\"so\"",
        "if [[ \"$OS\" == \"darwin\" ]]; then",
        "  LIB_EXT=\"dylib\"",
        "elif [[ \"$OS\" == \"mingw\"* || \"$OS\" == \"msys\"* || \"$OS\" == \"cygwin\"* ]]; then",
        "  LIB_EXT=\"dll\"",
        "fi",
        "",
        "LIB=\"$OUT_DIR/libexample_cpp_nodes.$LIB_EXT\"",
        "",
        "echo \"[c_cpp] building $LIB\"",
        "c++ -std=c++17 -O2 -fPIC -shared -I\"$ROOT/sdk\" \"$SRC\" -o \"$LIB\"",
        "",
        "MANIFEST=\"$OUT_DIR/example_cpp.manifest.json\"",
        "export LIB",
        "export MANIFEST",
        "",
        "# Copy shader assets next to the manifest/library so `src_path` resolves.",
        "mkdir -p \"$OUT_DIR/shaders\"",
        "cp -f \"$SHADERS_DIR/\"*.wgsl \"$OUT_DIR/shaders/\" 2>/dev/null || true",
        "",
        "# Emit a manifest file by calling the dylib's exported `daedalus_cpp_manifest` symbol, then",
        "# patch in cc_path to point back at this dylib (the \"manifest file\" flow).",
        "python - <<'PY'",
        "import ctypes",
        "import json",
        "import os",
        "from pathlib import Path",
        "",
        "lib_path = Path(os.environ[\"LIB\"]).resolve()",
        "out = Path(os.environ[\"MANIFEST\"]).resolve()",
        "out.parent.mkdir(parents=True, exist_ok=True)",
        "",
        "class Result(ctypes.Structure):",
        "    _fields_ = [(\"json\", ctypes.c_char_p), (\"error\", ctypes.c_char_p)]",
        "",
        "lib = ctypes.CDLL(str(lib_path))",
        "mf = lib.daedalus_cpp_manifest",
        "mf.restype = Result",
        "free = lib.daedalus_free",
        "free.argtypes = [ctypes.c_void_p]",
        "",
        "res = mf()",
        "if res.error:",
        "    err = ctypes.string_at(res.error).decode(\"utf-8\", errors=\"replace\")",
        "    free(res.error)",
        "    raise SystemExit(err)",
        "if not res.json:",
        "    raise SystemExit(\"daedalus_cpp_manifest returned null\")",
        "",
        "json_str = ctypes.string_at(res.json).decode(\"utf-8\", errors=\"replace\")",
        "free(res.json)",
        "",
        "doc = json.loads(json_str)",
        "doc[\"language\"] = \"c_cpp\"",
        "for n in doc.get(\"nodes\", []):",
        "    n.setdefault(\"cc_path\", lib_path.name)",
        "    n.setdefault(\"cc_free\", \"daedalus_free\")",
        "",
        "out.write_text(json.dumps(doc, indent=2) + \"\\n\", encoding=\"utf-8\")",
        "print(out.as_posix())",
        "PY",
        "",
        "echo \"[c_cpp] wrote $MANIFEST (and $LIB exports daedalus_cpp_manifest for manifest-less loading)\"",
        "",
    ]
    .join("\n");
    fs::write(&build_path, script)?;
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&build_path)?.permissions();
        perms.set_mode(0o755);
        let _ = fs::set_permissions(&build_path, perms);
    }
    Ok(())
}
