# lib-cv

This crate houses lightweight computer vision utilities used by the pipeline. Modules provide contour extraction, drawing helpers, ArUco marker support and convenience wrappers around common image operations. Optional features enable or disable groups of functionality such as `contour`, `draw`, `aruco` and experimental AI helpers. All algorithms operate on `image` crate buffers and can leverage rayon for parallelism.

Performance critical image routines should attempt to parallelize work using `rayon` or similar techniques whenever possible.

Data types used by nodes in this crate should remain generic. Prefer composing existing primitives like `Point` and `Vec` instead of introducing bespoke types for a specific node output.

## Performance Guidelines
- Avoid cloning or converting images when it is not required. Most helpers operate directly on `GrayImage`/`RgbImage` buffers.
- The `resize` module exposes a thread-local `Resizer` to reuse internal buffers across calls. Use this helper instead of constructing a new `Resizer` each time.
- Heavy pixel loops should use `rayon` to parallelize work and fully utilize available CPU cores.
