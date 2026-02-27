# lib-cv

`lib-cv` aggregates computer-vision building blocks (filters, morphology, contour analysis, drawing) and exposes them as pipeline nodes via the shared `NodeRegistry` API.

## Module Catalog

The registry is assembled from a set of modules. Each module advertises its metadata through `ModuleInfo`:

| Module ID    | Description                    | Feature Gate |
|--------------|--------------------------------|--------------|
| `cv.filter`  | Spatial filtering primitives   | –            |
| `cv.transform` | Geometric transforms        | –            |
| `cv.color`   | Color space operations         | –            |
| `cv.draw`    | Rendering primitives           | `draw`       |
| `cv.pixel`   | Pixel read/write helpers       | –            |
| `cv.analysis`| Contour analysis utilities     | `contour`    |
| `cv.morphology` | Morphological operators    | –            |
| `cv.motion`  | Motion detection nodes         | –            |

You can inspect the active modules at runtime:

```rust
use lib_cv::CV_NODE_REGISTRY;

for module in CV_NODE_REGISTRY.modules() {
    println!("{} v{} ({:?})", module.id, module.version, module.features);
}
```

## Building a Registry

The static `CV_NODE_REGISTRY` is built via the new `CvRegistryBuilder`, which is also available if you need a customised subset of modules:

```rust
use lib_cv::nodes::{CvRegistryBuilder, CvNodeModule};
use lib_cv::nodes::filter::blur;

let mut builder = CvRegistryBuilder::default();
// register only filter + pixel modules
builder.add_module(lib_cv::nodes::FilterModule);
builder.add_module(lib_cv::nodes::PixelModule);
let registry = builder.build();
assert!(registry.registry().list_nodes().iter().any(|id| id == "cv:blur"));
```

## Feature Flags

Feature flags map directly to modules:

- `draw` – enables the drawing module (`cv.draw`).
- `contour` – enables contour analysis nodes (`cv.analysis`); combine with `aruco` to expose `cv:decode_grid`.
- `aruco` – enables ArUco tag helpers used by the contour decoder.
- `ai` – reserved for future DNN-based operators.
- `gpu` – enables the experimental GPU acceleration hooks. When active you can
  register a device backend with `lib_cv::install_gpu_backend` so selected
  filter and morphology nodes can delegate work to GPU kernels.

The default feature set is `all`, which pulls in every module.

## Backend Selection

Many filter and morphology nodes expose a new `mode` input typed as the
`ExecutionMode` enum (`auto`, `cpu`, `simd`, `gpu`). Existing graphs can leave
this input at the default `auto`, while advanced graphs can now force a specific
backend without wiring separate SIMD/GPU-specific nodes.

## Testing

Run the full module suite with:

```bash
cargo test -p lib-cv --all-features
```

Individual module tests live under `src/libs/lib-cv/tests/`.

## Benchmarks

Micro-benchmarks for the highest-traffic nodes live under `benches/` and use
Criterion. Run them with:

```bash
cargo bench -p lib-cv --bench filter_bench
```
