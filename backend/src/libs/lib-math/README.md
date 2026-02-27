# lib-math

Helios math primitives spanning configurable runtime filters, vector/matrix helpers and quaternion utilities. The crate feeds the capture, vision and sensors stacks so we can model dynamics and pose consistently across binaries.

## What lives here?
- **Filters** – low pass, high pass, Kalman family, particle filter and complementary fusers. Every filter now exposes a serialisable `Config` payload so higher layers can persist and reload tuning knobs.
- **Linear algebra** – lightweight `Vec3`/`Vec4`, `Mat3`/`Mat4` and `Quaternion` types with helpers for dot/cross products, inverses and axis-angle conversions.

## Using filters
```rust
use lib_math::{LowPassConfig, LowPassFilter, Filter};

let mut filter = LowPassFilter::<1>::from_config(LowPassConfig { alpha: 0.3 })?;
let current = filter.update([42.0]);
println!("smoothed value: {current:?}");
```

All filters support `from_config`, `config` and `reconfigure` in addition to the runtime update/value APIs.

## Numerical stability
- Kalman family constructors validate that process/measurement noise terms are positive to avoid singular covariance matrices.
- Quaternion helpers reject zero-magnitude axes and normalise before converting to matrices to avoid drift.
- Matrix inversion reports `LinalgError::SingularMatrix` when the determinant is close to zero instead of returning garbage.

## Examples
Run the crate examples directly:

```bash
cargo run -p lib-math --example smoothing
```

## Benchmarks & property tests
- QuickCheck properties exercise low-pass stability and configuration round-tripping.
- Criterion benches live under `benches/` and focus on hot filter paths. Run them with `cargo bench -p lib-math`.

## Minimum Supported Rust Version (MSRV)
The crate targets **Rust 1.79** or newer in line with the Helios workspace policy. CI enforces this before publishing the support crates.

