use libloading::Library;
use std::env;
use std::ffi::{CStr, c_char, c_void};
use std::path::{Path, PathBuf};
use std::process::Command;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeliosDataType {
    Unit = 0,
    Bool = 1,
    Uint = 2,
    Sint = 3,
    Float = 4,
    Double = 5,
    String = 6,
    Bytes = 7,
    Image = 8,
    Pixel = 9,
    Point = 10,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct HeliosStr {
    ptr: *const c_char,
    len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct HeliosBuffer {
    ptr: *const u8,
    len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct HeliosImage {
    data: *mut u8,
    width: u32,
    height: u32,
    channels: u8,
    stride: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct HeliosPixel {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct HeliosPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
union HeliosValueData {
    unit: u8,
    boolean: bool,
    uint: u32,
    sint: i32,
    float32: f32,
    float64: f64,
    string: HeliosStr,
    bytes: HeliosBuffer,
    image: HeliosImage,
    pixel: HeliosPixel,
    point: HeliosPoint,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct HeliosValue {
    data_type: HeliosDataType,
    value: HeliosValueData,
}

#[repr(C)]
struct HeliosPort {
    name: *const c_char,
    description: *const c_char,
    data_type: HeliosDataType,
    type_expr_json: *const c_char,
    default_json: *const c_char,
    has_min: bool,
    min: f64,
    has_max: bool,
    max: f64,
}

#[repr(C)]
struct HeliosNodeDescriptor {
    id: *const c_char,
    name: *const c_char,
    summary: *const c_char,
    description: *const c_char,
    latency_class: u16,
    tags: *const *const c_char,
    tag_count: usize,
    categories: *const *const c_char,
    category_count: usize,
    align_inputs: *const *const c_char,
    align_input_count: usize,
    inputs: *const HeliosPort,
    input_count: usize,
    outputs: *const HeliosPort,
    output_count: usize,
}

type CreateFn = unsafe extern "C" fn(*const HeliosNodeDescriptor) -> *mut c_void;
type StepFn = unsafe extern "C" fn(*mut c_void, *const HeliosValue, usize, *mut HeliosValue, usize) -> i32;
type DestroyFn = unsafe extern "C" fn(*mut c_void);
type ReleaseFn = unsafe extern "C" fn(*const u8, usize);

#[repr(C)]
struct HeliosNodeVTable {
    create: Option<CreateFn>,
    step: Option<StepFn>,
    destroy: Option<DestroyFn>,
}

#[repr(C)]
struct HeliosNodeDefinition {
    descriptor: HeliosNodeDescriptor,
    vtable: HeliosNodeVTable,
}

#[repr(C)]
struct HeliosNodeRegistry {
    abi_version: u32,
    name: *const c_char,
    nodes: *const HeliosNodeDefinition,
    node_count: usize,
    release_buffer: Option<ReleaseFn>,
}

type RegistryFn = unsafe extern "C" fn() -> *const HeliosNodeRegistry;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().expect("resolve repo root")
}

fn sdk_root(root: &Path) -> PathBuf {
    root.join("helios-node-sdk")
}

fn has_helios_sdk(root: &Path) -> bool {
    sdk_root(root).is_dir()
}

fn tool_available(program: &str) -> bool {
    Command::new(program).arg("--version").output().is_ok()
}

fn skip_missing_sdk(root: &Path, test_name: &str) -> bool {
    if has_helios_sdk(root) {
        return false;
    }
    eprintln!("skipping {test_name}: missing helios-node-sdk checkout at {}", sdk_root(root).display());
    true
}

fn java_home() -> Option<PathBuf> {
    if let Ok(val) = env::var("JAVA_HOME") {
        return Some(PathBuf::from(val));
    }
    if let Ok(output) = Command::new("java").arg("-XshowSettings:properties").arg("-version").output() {
        let text = String::from_utf8_lossy(&output.stderr);
        for line in text.lines() {
            if let Some(rest) = line.trim().strip_prefix("java.home =") {
                return Some(PathBuf::from(rest.trim()));
            }
        }
    }
    None
}

fn run(mut cmd: Command, name: &str) {
    let output = cmd.output().unwrap_or_else(|err| panic!("{name} failed to start: {err}"));
    if !output.status.success() {
        panic!("{name} failed: {}\nstdout:\n{}\nstderr:\n{}", output.status, String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    }
}

fn sample_values() -> Vec<HeliosValue> {
    let bytes_data = vec![1u8, 2, 3];
    let bytes_ptr = bytes_data.as_ptr();
    let bytes_len = bytes_data.len();
    std::mem::forget(bytes_data);

    let mut image_buf = vec![10u8; 4];
    let img_ptr = image_buf.as_mut_ptr();
    std::mem::forget(image_buf);

    vec![
        HeliosValue { data_type: HeliosDataType::Unit, value: HeliosValueData { unit: 0 } },
        HeliosValue { data_type: HeliosDataType::Bool, value: HeliosValueData { boolean: true } },
        HeliosValue { data_type: HeliosDataType::Uint, value: HeliosValueData { uint: 7 } },
        HeliosValue { data_type: HeliosDataType::Sint, value: HeliosValueData { sint: -3 } },
        HeliosValue { data_type: HeliosDataType::Float, value: HeliosValueData { float32: 1.5 } },
        HeliosValue { data_type: HeliosDataType::Double, value: HeliosValueData { float64: 2.5 } },
        HeliosValue { data_type: HeliosDataType::String, value: HeliosValueData { string: HeliosStr { ptr: c"hello".as_ptr(), len: c"hello".to_bytes().len() } } },
        HeliosValue { data_type: HeliosDataType::Bytes, value: HeliosValueData { bytes: HeliosBuffer { ptr: bytes_ptr, len: bytes_len } } },
        HeliosValue { data_type: HeliosDataType::Image, value: HeliosValueData { image: HeliosImage { data: img_ptr, width: 1, height: 1, channels: 4, stride: 4 } } },
        HeliosValue { data_type: HeliosDataType::Pixel, value: HeliosValueData { pixel: HeliosPixel { r: 1, g: 2, b: 3, a: 4 } } },
        HeliosValue { data_type: HeliosDataType::Point, value: HeliosValueData { point: HeliosPoint { x: 1.0, y: 2.0 } } },
    ]
}

#[allow(unsafe_code)]
fn assert_roundtrip(outputs: &[HeliosValue]) {
    assert_eq!(outputs.len(), 11);
    unsafe {
        assert_eq!(outputs[0].data_type, HeliosDataType::Unit);
        assert!(outputs[1].value.boolean);
        assert_eq!(outputs[2].value.uint, 7);
        assert_eq!(outputs[3].value.sint, -3);
        assert!((outputs[4].value.float32 - 1.5).abs() < 1e-6);
        assert!((outputs[5].value.float64 - 2.5).abs() < 1e-9);
        let s = CStr::from_ptr(outputs[6].value.string.ptr);
        assert_eq!(s.to_str().unwrap(), "hello");
        assert_eq!(outputs[7].value.bytes.len, 3);
        let img = outputs[8].value.image;
        assert_eq!(img.width, 1);
        assert_eq!(outputs[9].value.pixel.r, 1);
        assert_eq!(outputs[10].value.point.x, 1.0);
    }
}

#[allow(unsafe_code)]
fn run_roundtrip(lib_path: &PathBuf, expected_id: &str) {
    unsafe {
        let lib = Library::new(lib_path).unwrap_or_else(|e| panic!("load {:?}: {e}", lib_path));
        let reg_fn: libloading::Symbol<RegistryFn> = lib.get(b"helios_node_registry").expect("registry export");
        let reg = reg_fn();
        assert!(!reg.is_null(), "registry null");
        let reg = &*reg;
        assert_eq!(reg.abi_version, 1);
        let defs = std::slice::from_raw_parts(reg.nodes, reg.node_count);
        let def = defs.iter().find(|d| CStr::from_ptr(d.descriptor.id).to_str().unwrap() == expected_id).expect("node present");

        let create = def.vtable.create;
        let destroy = def.vtable.destroy;
        let step = def.vtable.step.expect("step");
        let handle = match create {
            Some(f) => f(&def.descriptor),
            None => std::ptr::null_mut(),
        };
        let inputs = sample_values();
        let mut outputs = vec![HeliosValue { data_type: HeliosDataType::Unit, value: HeliosValueData { unit: 0 } }; 11];
        let rc = step(handle, inputs.as_ptr(), inputs.len(), outputs.as_mut_ptr(), outputs.len());
        assert_eq!(rc, 0, "step failed");
        assert_roundtrip(&outputs);
        if let Some(d) = destroy {
            d(handle);
        }

        if let Some(release) = reg.release_buffer {
            if outputs[7].data_type == HeliosDataType::Bytes && !outputs[7].value.bytes.ptr.is_null() {
                release(outputs[7].value.bytes.ptr, outputs[7].value.bytes.len);
            }
            if outputs[8].data_type == HeliosDataType::Image && !outputs[8].value.image.data.is_null() {
                let len = (outputs[8].value.image.height as usize).saturating_mul(outputs[8].value.image.stride as usize);
                release(outputs[8].value.image.data, len);
            }
            if outputs[6].data_type == HeliosDataType::String && !outputs[6].value.string.ptr.is_null() {
                let len = outputs[6].value.string.len;
                release(outputs[6].value.string.ptr as *const u8, len);
            }
        }
    }
}

#[test]
fn sdk_rust_tests() {
    let root = repo_root();
    if skip_missing_sdk(&root, "sdk_rust_tests") {
        return;
    }
    let mut cmd = Command::new("cargo");
    cmd.arg("test").arg("-p").arg("helios-node-sdk-rust").arg("--quiet").current_dir(&root);
    run(cmd, "rust sdk tests");

    let mut build = Command::new("cargo");
    build.arg("build").arg("-p").arg("helios-node-sdk-rust").arg("--release").current_dir(&root);
    run(build, "rust sdk build");
    let lib_path = root.join("helios-node-sdk/rust/target/release").join(if cfg!(target_os = "macos") {
        "libhelios_node_sdk_rust.dylib"
    } else if cfg!(target_os = "windows") {
        "helios_node_sdk_rust.dll"
    } else {
        "libhelios_node_sdk_rust.so"
    });
    run_roundtrip(&lib_path, "test:all_types_rust");
}

fn find_python() -> String {
    for candidate in ["python3", "python"] {
        if Command::new(candidate).arg("--version").output().is_ok() {
            return candidate.to_string();
        }
    }
    panic!("python interpreter not found");
}

#[test]
fn sdk_python_tests() {
    let root = repo_root();
    if skip_missing_sdk(&root, "sdk_python_tests") {
        return;
    }
    if !tool_available("python3") && !tool_available("python") {
        eprintln!("skipping sdk_python_tests: python interpreter not available");
        return;
    }
    let mut cmd = Command::new(find_python());
    cmd.arg("-m").arg("pytest").arg("helios-node-sdk/python/tests").current_dir(&root);
    run(cmd, "python sdk tests");
}

#[test]
fn sdk_python_roundtrip() {
    let root = repo_root();
    if skip_missing_sdk(&root, "sdk_python_roundtrip") {
        return;
    }
    if !tool_available("python3") && !tool_available("python") {
        eprintln!("skipping sdk_python_roundtrip: python interpreter not available");
        return;
    }
    let out_dir = root.join("target/sdk-smoke");
    std::fs::create_dir_all(&out_dir).expect("create temp dir");
    let script = r#"
	import sys, os
root = os.path.abspath(".")
sys.path.insert(0, os.path.join(root, "helios-node-sdk/python"))
sys.path.insert(0, os.path.join(root, "helios-node-sdk/python/examples"))
from helios_sdk import build_plugin
from all_types import AllTypesPlugin
path = build_plugin(AllTypesPlugin, module_name="all_types_py_plugin")
print(path)
"#;
    let mut cmd = Command::new(find_python());
    cmd.arg("-c").arg(script).current_dir(&root);
    let output = cmd.output().expect("run python build");
    if !output.status.success() {
        panic!("python build failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    let path_str = String::from_utf8_lossy(&output.stdout).lines().last().unwrap_or("").trim().to_string();
    let lib_path = PathBuf::from(path_str);
    run_roundtrip(&lib_path, "test:all_types_py");
}

#[test]
fn sdk_c_and_cpp_build() {
    let root = repo_root();
    if skip_missing_sdk(&root, "sdk_c_and_cpp_build") {
        return;
    }
    if !tool_available("gcc") || !tool_available("g++") {
        eprintln!("skipping sdk_c_and_cpp_build: missing gcc/g++ toolchain");
        return;
    }
    let out_dir = root.join("target/sdk-smoke");
    std::fs::create_dir_all(&out_dir).expect("create temp dir");

    let mut gcc = Command::new("gcc");
    gcc.arg("-fPIC")
        .arg("-shared")
        .arg("-I")
        .arg(root.join("helios-node-sdk/c"))
        .arg(root.join("helios-node-sdk/c/examples/all_types.c"))
        .arg("-o")
        .arg(out_dir.join("libhelios_c_sdk.so"))
        .current_dir(&root);
    run(gcc, "c sdk build");

    let mut gpp = Command::new("g++");
    gpp.arg("-std=c++17")
        .arg("-fPIC")
        .arg("-shared")
        .arg("-I")
        .arg(root.join("helios-node-sdk/c"))
        .arg("-I")
        .arg(root.join("helios-node-sdk/cpp"))
        .arg(root.join("helios-node-sdk/cpp/examples/all_types.cpp"))
        .arg("-o")
        .arg(out_dir.join("libhelios_cpp_sdk.so"))
        .current_dir(&root);
    run(gpp, "cpp sdk build");

    run_roundtrip(&out_dir.join("libhelios_c_sdk.so"), "test:all_types_c");
    run_roundtrip(&out_dir.join("libhelios_cpp_sdk.so"), "test:all_types_cpp");
}

#[test]
fn sdk_java_registry_smoke() {
    let root = repo_root();
    if skip_missing_sdk(&root, "sdk_java_registry_smoke") {
        return;
    }
    if !tool_available("javac") || !tool_available("java") {
        eprintln!("skipping sdk_java_registry_smoke: missing java/javac");
        return;
    }
    let out_dir = root.join("target/sdk-smoke/java");
    std::fs::create_dir_all(&out_dir).expect("create java output dir");
    let classpath =
        format!("{}:{}:{}", root.join("helios-node-sdk/java/src/main/java").display(), root.join("helios-node-sdk/java/examples").display(), root.join("helios-node-sdk/java/tests").display());

    let mut javac = Command::new("javac");
    javac.arg("-d").arg(&out_dir).arg("-cp").arg(&classpath).arg(root.join("helios-node-sdk/java/tests/RegistrySmoke.java")).current_dir(&root);
    run(javac, "javac sdk registry smoke");

    let mut java = Command::new("java");
    java.arg("-cp").arg(format!("{out}:{cp}", out = out_dir.display(), cp = classpath)).arg("RegistrySmoke").current_dir(&root);
    run(java, "java sdk registry smoke");

    // Build JNI plugin and round-trip through ABI
    let jhome = java_home().expect("JAVA_HOME not found and could not infer");
    let include = jhome.join("include");
    let include_os = include.join(if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "win32"
    } else {
        "linux"
    });
    let libjvm_dir = jhome.join("lib/server");
    let cp_var = format!("{out}:{cp}", out = out_dir.display(), cp = classpath);
    let mut gcc = Command::new("gcc");
    let out_lib = out_dir.join("libhelios_java_sdk.so");
    gcc.arg("-shared")
        .arg("-fPIC")
        .arg("-I")
        .arg(root.join("helios-node-sdk/c"))
        .arg("-I")
        .arg(&include)
        .arg("-I")
        .arg(&include_os)
        .arg(format!("-DJAVA_TEST_CLASSPATH_VALUE=\"{}\"", cp_var))
        .arg(root.join("helios-node-sdk/java/native.c"))
        .arg("-L")
        .arg(&libjvm_dir)
        .arg("-ljvm")
        .arg("-o")
        .arg(&out_lib)
        .current_dir(&root);
    run(gcc, "java sdk native build");

    run_roundtrip(&out_lib, "test:all_types_java");
}
