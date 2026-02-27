use super::*;

static UNDISTORT_OPTIONAL_LOGGED_OK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static UNDISTORT_OPTIONAL_LOGGED_PARSE_FAIL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Debug, Clone, Copy)]
struct UndistortCalib {
    in_fx: f32,
    in_fy: f32,
    in_cx: f32,
    in_cy: f32,
    out_fx: f32,
    out_fy: f32,
    out_cx: f32,
    out_cy: f32,
    k1: f32,
    k2: f32,
    p1: f32,
    p2: f32,
    k3: f32,
    lens_model: crate::modules::calibration::LensModel,
}

fn distort_norm_pinhole(x: f32, y: f32, calib: UndistortCalib) -> (f32, f32) {
    let r2 = x * x + y * y;
    let r4 = r2 * r2;
    let r6 = r4 * r2;
    let radial = 1.0 + calib.k1 * r2 + calib.k2 * r4 + calib.k3 * r6;
    let xy2 = 2.0 * x * y;
    let x2 = x * x;
    let y2 = y * y;
    let xt = calib.p1 * xy2 + calib.p2 * (r2 + 2.0 * x2);
    let yt = calib.p1 * (r2 + 2.0 * y2) + calib.p2 * xy2;
    (x * radial + xt, y * radial + yt)
}

fn distort_norm_fisheye(x: f32, y: f32, calib: UndistortCalib) -> (f32, f32) {
    let r = (x * x + y * y).sqrt();
    if !r.is_finite() || r <= 1e-6 {
        return (x, y);
    }
    let theta = r.atan();
    let theta2 = theta * theta;
    let theta4 = theta2 * theta2;
    let theta6 = theta4 * theta2;
    let theta8 = theta4 * theta4;
    let theta10 = theta8 * theta2;
    let k4 = calib.p1;
    let k5 = calib.p2;
    let theta_d = theta * (1.0 + calib.k1 * theta2 + calib.k2 * theta4 + calib.k3 * theta6 + k4 * theta8 + k5 * theta10);
    let scale = theta_d / r;
    (x * scale, y * scale)
}

fn distort_norm(x: f32, y: f32, calib: UndistortCalib) -> (f32, f32) {
    match calib.lens_model {
        crate::modules::calibration::LensModel::Pinhole => distort_norm_pinhole(x, y, calib),
        crate::modules::calibration::LensModel::Fisheye => distort_norm_fisheye(x, y, calib),
    }
}

fn in_bounds_sample(width: u32, height: u32, sx: f32, sy: f32) -> bool {
    if !(sx.is_finite() && sy.is_finite()) {
        return false;
    }
    if width == 0 || height == 0 {
        return false;
    }
    let max_x = (width - 1) as f32;
    let max_y = (height - 1) as f32;
    sx >= 0.0 && sy >= 0.0 && sx <= max_x && sy <= max_y
}

fn undistort_fill_valid(width: u32, height: u32, calib: UndistortCalib, samples_per_edge: u32) -> bool {
    let w = width.max(1);
    let h = height.max(1);
    let n = samples_per_edge.max(4);
    let max_x = w - 1;
    let max_y = h - 1;

    let step_x = (max_x as f32) / ((n - 1) as f32);
    let step_y = (max_y as f32) / ((n - 1) as f32);

    // Sample along all 4 borders. Corners are included by construction.
    for i in 0..n {
        let xf = (i as f32) * step_x;
        let yf = (i as f32) * step_y;

        for (x, y) in [(xf, 0.0f32), (xf, max_y as f32), (0.0f32, yf), (max_x as f32, yf)] {
            let xu = (x - calib.out_cx) / calib.out_fx;
            let yu = (y - calib.out_cy) / calib.out_fy;
            let (xd, yd) = distort_norm(xu, yu, calib);
            let sx = calib.in_fx * xd + calib.in_cx;
            let sy = calib.in_fy * yd + calib.in_cy;
            if !in_bounds_sample(w, h, sx, sy) {
                return false;
            }
        }
    }
    true
}

fn compute_zoom_fill(width: u32, height: u32, mut calib: UndistortCalib) -> f32 {
    // Bracket the transition between "has black borders" and "fully filled".
    // Increasing zoom reduces FOV and makes the mapping more in-bounds.
    let samples = 48u32;
    let mut hi = 1.0f32;
    for _ in 0..16 {
        calib.out_fx = calib.in_fx * hi;
        calib.out_fy = calib.in_fy * hi;
        if undistort_fill_valid(width, height, calib, samples) {
            break;
        }
        hi *= 1.5;
    }

    // If even a huge zoom can't fill, fall back to "no-op zoom".
    calib.out_fx = calib.in_fx * hi;
    calib.out_fy = calib.in_fy * hi;
    if !undistort_fill_valid(width, height, calib, samples) {
        return 1.0;
    }

    let mut lo = hi;
    for _ in 0..20 {
        let next = lo * 0.5;
        if next < 0.02 {
            break;
        }
        calib.out_fx = calib.in_fx * next;
        calib.out_fy = calib.in_fy * next;
        if !undistort_fill_valid(width, height, calib, samples) {
            break;
        }
        lo = next;
    }

    // If `lo` is still valid, it's already the smallest we tried.
    calib.out_fx = calib.in_fx * lo;
    calib.out_fy = calib.in_fy * lo;
    if undistort_fill_valid(width, height, calib, samples) {
        return lo;
    }

    // Now we have `lo` invalid, `hi` valid.
    let mut lo_bad = lo;
    let mut hi_good = hi;
    for _ in 0..24 {
        let mid = (lo_bad + hi_good) * 0.5;
        calib.out_fx = calib.in_fx * mid;
        calib.out_fy = calib.in_fy * mid;
        if undistort_fill_valid(width, height, calib, samples) {
            hi_good = mid;
        } else {
            lo_bad = mid;
        }
    }
    hi_good
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct UndistortFillZoomKey {
    width: u32,
    height: u32,
    lens_model: u8,
    in_fx: u32,
    in_fy: u32,
    in_cx: u32,
    in_cy: u32,
    out_cx: u32,
    out_cy: u32,
    k1: u32,
    k2: u32,
    p1: u32,
    p2: u32,
    k3: u32,
}

fn undistort_fill_zoom_key(width: u32, height: u32, calib: UndistortCalib) -> UndistortFillZoomKey {
    let lens_model = match calib.lens_model {
        crate::modules::calibration::LensModel::Pinhole => 0,
        crate::modules::calibration::LensModel::Fisheye => 1,
    };
    UndistortFillZoomKey {
        width,
        height,
        lens_model,
        in_fx: calib.in_fx.to_bits(),
        in_fy: calib.in_fy.to_bits(),
        in_cx: calib.in_cx.to_bits(),
        in_cy: calib.in_cy.to_bits(),
        out_cx: calib.out_cx.to_bits(),
        out_cy: calib.out_cy.to_bits(),
        k1: calib.k1.to_bits(),
        k2: calib.k2.to_bits(),
        p1: calib.p1.to_bits(),
        p2: calib.p2.to_bits(),
        k3: calib.k3.to_bits(),
    }
}

fn compute_zoom_fill_cached(width: u32, height: u32, calib: UndistortCalib) -> f32 {
    type FillZoomCache = RwLock<Option<(u64, f32)>>;
    static LAST: OnceLock<FillZoomCache> = OnceLock::new();
    let storage = LAST.get_or_init(|| RwLock::new(None));

    let key = undistort_fill_zoom_key(width, height, calib);
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    let key_hash = hasher.finish();

    if let Ok(guard) = storage.read()
        && let Some((prev_hash, zoom)) = guard.as_ref()
        && *prev_hash == key_hash
    {
        return *zoom;
    }

    let zoom = compute_zoom_fill(width, height, calib);
    if let Ok(mut guard) = storage.write() {
        *guard = Some((key_hash, zoom));
    }
    zoom
}

fn sample_bilinear(src: &[u8], width: usize, height: usize, channels: usize, x: f32, y: f32, border_clamp: bool, out: &mut [u8]) {
    if channels == 0 || width == 0 || height == 0 {
        out.fill(0);
        return;
    }

    let max_x = (width - 1) as f32;
    let max_y = (height - 1) as f32;
    let (x, y) = if border_clamp { (x.clamp(0.0, max_x), y.clamp(0.0, max_y)) } else { (x, y) };
    if !border_clamp && (x < 0.0 || y < 0.0 || x > max_x || y > max_y) {
        out.fill(0);
        return;
    }

    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let x1 = (x0 + 1).min(width as i32 - 1);
    let y1 = (y0 + 1).min(height as i32 - 1);

    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let w00 = (1.0 - fx) * (1.0 - fy);
    let w10 = fx * (1.0 - fy);
    let w01 = (1.0 - fx) * fy;
    let w11 = fx * fy;

    let idx = |xx: i32, yy: i32| -> usize { (yy as usize * width + xx as usize) * channels };
    let i00 = idx(x0, y0);
    let i10 = idx(x1, y0);
    let i01 = idx(x0, y1);
    let i11 = idx(x1, y1);

    for c in 0..channels.min(out.len()) {
        let v00 = src.get(i00 + c).copied().unwrap_or(0) as f32;
        let v10 = src.get(i10 + c).copied().unwrap_or(0) as f32;
        let v01 = src.get(i01 + c).copied().unwrap_or(0) as f32;
        let v11 = src.get(i11 + c).copied().unwrap_or(0) as f32;
        out[c] = (v00 * w00 + v10 * w10 + v01 * w01 + v11 * w11).round().clamp(0.0, 255.0) as u8;
    }
}

#[derive(Debug, Clone)]
struct UndistortMap {
    width: u32,
    height: u32,
    border_clamp: bool,
    coords: Arc<[f32]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct UndistortMapKey {
    width: u32,
    height: u32,
    border_clamp: bool,
    lens_model: u8,
    in_fx: u32,
    in_fy: u32,
    in_cx: u32,
    in_cy: u32,
    out_fx: u32,
    out_fy: u32,
    out_cx: u32,
    out_cy: u32,
    k1: u32,
    k2: u32,
    p1: u32,
    p2: u32,
    k3: u32,
}

fn undistort_key(width: u32, height: u32, calib: UndistortCalib, border_clamp: bool) -> UndistortMapKey {
    let lens_model = match calib.lens_model {
        crate::modules::calibration::LensModel::Pinhole => 0,
        crate::modules::calibration::LensModel::Fisheye => 1,
    };
    UndistortMapKey {
        width,
        height,
        border_clamp,
        lens_model,
        in_fx: calib.in_fx.to_bits(),
        in_fy: calib.in_fy.to_bits(),
        in_cx: calib.in_cx.to_bits(),
        in_cy: calib.in_cy.to_bits(),
        out_fx: calib.out_fx.to_bits(),
        out_fy: calib.out_fy.to_bits(),
        out_cx: calib.out_cx.to_bits(),
        out_cy: calib.out_cy.to_bits(),
        k1: calib.k1.to_bits(),
        k2: calib.k2.to_bits(),
        p1: calib.p1.to_bits(),
        p2: calib.p2.to_bits(),
        k3: calib.k3.to_bits(),
    }
}

fn get_undistort_map(width: u32, height: u32, calib: UndistortCalib, border_clamp: bool) -> Arc<UndistortMap> {
    type UndistortCache = RwLock<Option<(u64, Arc<UndistortMap>)>>;
    static LAST: OnceLock<UndistortCache> = OnceLock::new();
    let storage = LAST.get_or_init(|| RwLock::new(None));

    let key = undistort_key(width, height, calib, border_clamp);
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    let key_hash = hasher.finish();

    if let Ok(guard) = storage.read() {
        if let Some((prev_hash, map)) = guard.as_ref() {
            if *prev_hash == key_hash && map.width == width && map.height == height && map.border_clamp == border_clamp {
                return map.clone();
            }
        }
    }

    let w = width.max(1) as usize;
    let h = height.max(1) as usize;
    let mut coords = vec![f32::NAN; w * h * 2];
    let max_x = (w - 1) as f32;
    let max_y = (h - 1) as f32;
    for y in 0..h {
        for x in 0..w {
            // Output pixel (rectified) -> normalized ray using the output intrinsics.
            let xu = (x as f32 - calib.out_cx) / calib.out_fx;
            let yu = (y as f32 - calib.out_cy) / calib.out_fy;
            let (xd, yd) = distort_norm(xu, yu, calib);
            // Normalized distorted -> input pixel using the input intrinsics.
            let mut sx = calib.in_fx * xd + calib.in_cx;
            let mut sy = calib.in_fy * yd + calib.in_cy;
            if border_clamp {
                sx = sx.clamp(0.0, max_x);
                sy = sy.clamp(0.0, max_y);
            } else if sx < 0.0 || sy < 0.0 || sx > max_x || sy > max_y {
                // Leave as NaN; caller will fill with 0.
                continue;
            }
            let idx = (y * w + x) * 2;
            coords[idx] = sx;
            coords[idx + 1] = sy;
        }
    }

    let map = Arc::new(UndistortMap { width, height, border_clamp, coords: coords.into() });
    if let Ok(mut guard) = storage.write() {
        *guard = Some((key_hash, map.clone()));
    }
    map
}

fn undistort_bytes_with_map(src: &[u8], width: u32, height: u32, channels: usize, map: &UndistortMap) -> Vec<u8> {
    let w = width.max(1) as usize;
    let h = height.max(1) as usize;
    let mut dst = vec![0u8; w * h * channels];
    let stride = w * channels;
    let coords = map.coords.as_ref();

    dst.par_chunks_mut(stride).enumerate().for_each(|(y, row)| {
        let mut pix = vec![0u8; channels.max(1)];
        let y = y.min(h.saturating_sub(1));
        for x in 0..w {
            let ci = (y * w + x) * 2;
            let sx = coords.get(ci).copied().unwrap_or(f32::NAN);
            let sy = coords.get(ci + 1).copied().unwrap_or(f32::NAN);
            if !sx.is_finite() || !sy.is_finite() {
                let off = x * channels;
                row[off..off + channels].fill(0);
                continue;
            }
            sample_bilinear(src, w, h, channels, sx, sy, map.border_clamp, &mut pix);
            let off = x * channels;
            row[off..off + channels].copy_from_slice(&pix[..channels]);
        }
    });

    dst
}

fn undistort_dynamic_image(img: DynamicImage, calib: UndistortCalib, border_clamp: bool) -> DynamicImage {
    let (width, height) = img.dimensions();
    if width == 0 || height == 0 {
        return DynamicImage::new_rgba8(width, height);
    }

    let map = get_undistort_map(width, height, calib, border_clamp);

    match img {
        DynamicImage::ImageLuma8(gray) => {
            let src = gray.as_raw();
            let dst = undistort_bytes_with_map(src, width, height, 1, &map);
            let out = GrayImage::from_raw(width, height, dst).unwrap_or_else(|| GrayImage::new(width, height));
            DynamicImage::ImageLuma8(out)
        }
        DynamicImage::ImageRgb8(rgb) => {
            let src = rgb.as_raw();
            let dst = undistort_bytes_with_map(src, width, height, 3, &map);
            let out = RgbImage::from_raw(width, height, dst).unwrap_or_else(|| RgbImage::new(width, height));
            DynamicImage::ImageRgb8(out)
        }
        DynamicImage::ImageRgba8(rgba) => {
            let src = rgba.as_raw();
            let dst = undistort_bytes_with_map(src, width, height, 4, &map);
            let out = RgbaImage::from_raw(width, height, dst).unwrap_or_else(|| RgbaImage::new(width, height));
            DynamicImage::ImageRgba8(out)
        }
        other => {
            let rgba = other.to_rgba8();
            let src = rgba.as_raw();
            let dst = undistort_bytes_with_map(src, width, height, 4, &map);
            let out = RgbaImage::from_raw(width, height, dst).unwrap_or_else(|| RgbaImage::new(width, height));
            DynamicImage::ImageRgba8(out)
        }
    }
}

#[node(
    id = "undistort",
    summary = "Undistort an image using calibration intrinsics.",
    description = "Applies a distortion model (pinhole or fisheye) to remap the input into an undistorted output (same resolution). Use `zoom` to trade FOV vs black borders: `zoom > 1` crops (less FOV), `zoom < 1` keeps more FOV (may add black borders).",
    inputs(
        "frame",
	            port(name = "fx", default = 0.0f64, meta(ui_min = 0.0, ui_max = 5000.0, ui_step = 1.0)),
	            port(name = "fy", default = 0.0f64, meta(ui_min = 0.0, ui_max = 5000.0, ui_step = 1.0)),
        port(name = "cx", default = 0.0f64, meta(ui_min = 0.0, ui_max = 5000.0, ui_step = 1.0)),
        port(name = "cy", default = 0.0f64, meta(ui_min = 0.0, ui_max = 5000.0, ui_step = 1.0)),
        port(name = "k1", default = 0.0f64, meta(ui_min = -1.0, ui_max = 1.0, ui_step = 0.001)),
        port(name = "k2", default = 0.0f64, meta(ui_min = -1.0, ui_max = 1.0, ui_step = 0.001)),
        port(name = "p1", default = 0.0f64, meta(ui_min = -1.0, ui_max = 1.0, ui_step = 0.001)),
        port(name = "p2", default = 0.0f64, meta(ui_min = -1.0, ui_max = 1.0, ui_step = 0.001)),
	            port(name = "k3", default = 0.0f64, meta(ui_min = -1.0, ui_max = 1.0, ui_step = 0.001)),
        port(name = "model", default = "pinhole"),
        // When true, clamp out-of-bounds sampling to the nearest pixel; otherwise fill with 0.
        port(name = "border_mode", default = "zero"),
        port(name = "zoom_mode", default = "manual"),
        port(name = "zoom", default = 1.0f64, meta(ui_min = 0.25, ui_max = 2.0, ui_step = 0.01)),
        // Only used when `zoom_mode = fill`. Values > 1.0 crop slightly more to reduce edge stretch.
        port(name = "fill_margin", default = 1.0f64, meta(ui_min = 1.0, ui_max = 1.5, ui_step = 0.01))
    ),
    outputs("frame")
)]
fn cv_undistort(
    frame: Payload<DynamicImage>,
    fx: f64,
    fy: f64,
    cx: f64,
    cy: f64,
    k1: f64,
    k2: f64,
    p1: f64,
    p2: f64,
    k3: f64,
    model: crate::modules::calibration::LensModel,
    border_mode: BorderMode,
    zoom_mode: UndistortZoomMode,
    zoom: f64,
    fill_margin: f64,
    _exec_ctx: &ExecutionContext,
) -> Result<Payload<DynamicImage>, NodeError> {
    let border_clamp = border_mode.is_clamp();
    let in_fx = fx as f32;
    let in_fy = fy as f32;
    let in_cx = cx as f32;
    let in_cy = cy as f32;
    let fill_margin = fill_margin.clamp(1.0, 10.0) as f32;
    let zoom_manual = zoom.clamp(0.01, 100.0) as f32;
    let calib = UndistortCalib {
        in_fx,
        in_fy,
        in_cx,
        in_cy,
        out_fx: 1.0,
        out_fy: 1.0,
        out_cx: in_cx,
        out_cy: in_cy,
        k1: k1 as f32,
        k2: k2 as f32,
        p1: p1 as f32,
        p2: p2 as f32,
        k3: k3 as f32,
        lens_model: model,
    };

    if !(calib.in_fx.is_finite() && calib.in_fy.is_finite() && calib.in_fx > 0.0 && calib.in_fy > 0.0) {
        return Ok(frame);
    }

    let img = expect_cpu(frame, "undistort", Some(_exec_ctx))?;
    let (width, height) = img.dimensions();
    let zoom_eff = match zoom_mode {
        UndistortZoomMode::Manual => zoom_manual,
        UndistortZoomMode::Fill => {
            let z = compute_zoom_fill_cached(width, height, calib);
            (z * fill_margin).max(0.01)
        }
    };
    let out_fx = in_fx * zoom_eff;
    let out_fy = in_fy * zoom_eff;
    if !(out_fx.is_finite() && out_fy.is_finite() && out_fx > 0.0 && out_fy > 0.0) {
        return Ok(Payload::Cpu(img));
    }
    let calib = UndistortCalib { out_fx, out_fy, ..calib };
    Ok(Payload::Cpu(undistort_dynamic_image(img, calib, border_clamp)))
}

#[node(
    id = "undistort_optional",
	        summary = "Undistort an image using calibration when provided.",
	        description = "Reads a camera calibration payload (fx/fy/cx/cy/k1..k3, lens model) and undistorts the image; if calibration is missing/invalid, passes through the input frame. Use `zoom` to trade FOV vs black borders.",
    inputs(
        "frame",
        // Accept camera calibration directly (host bridge provides this payload).
        port(name = "calibration", source = "Calibration", ty = crate::daedalus_types::camera_calibration()),
        port(name = "border_mode", default = "zero"),
        port(name = "zoom_mode", default = "manual"),
        port(name = "zoom", default = 1.0f64, meta(ui_min = 0.25, ui_max = 2.0, ui_step = 0.01)),
        port(name = "fill_margin", default = 1.0f64, meta(ui_min = 1.0, ui_max = 1.5, ui_step = 0.01))
    ),
    outputs("frame")
)]
fn cv_undistort_optional(
    frame: Payload<DynamicImage>,
    calibration: Option<daedalus::data::model::Value>,
    border_mode: BorderMode,
    // Treat `zoom_mode` as optional at runtime to remain compatible with older graphs/hosts
    // that don't wire or const-bind the port (Daedalus may not apply node defaults here).
    zoom_mode: Option<UndistortZoomMode>,
    zoom: f64,
    fill_margin: f64,
    _exec_ctx: &ExecutionContext,
) -> Result<Payload<DynamicImage>, NodeError> {
    let border_clamp = border_mode.is_clamp();
    let zoom_mode = zoom_mode.unwrap_or(UndistortZoomMode::Manual);
    let Some(calibration) = calibration else {
        return Ok(frame);
    };
    let calibration = {
        use daedalus::data::model::{EnumValue as DaedalusEnumValue, StructFieldValue as DaedalusStructFieldValue, Value as DaedalusValue};

        fn get_field<'a>(fields: &'a [DaedalusStructFieldValue], name: &str) -> Option<&'a DaedalusValue> {
            fields.iter().find(|f| f.name.eq_ignore_ascii_case(name)).map(|f| &f.value)
        }

        fn as_f32(v: &DaedalusValue) -> Option<f32> {
            match v {
                DaedalusValue::Float(f) => Some(*f as f32),
                DaedalusValue::Int(i) => Some(*i as f32),
                _ => None,
            }
        }

        fn as_u8(v: &DaedalusValue) -> Option<u8> {
            match v {
                DaedalusValue::Int(i) => u8::try_from(*i).ok(),
                DaedalusValue::Float(f) => u8::try_from(*f as i64).ok(),
                _ => None,
            }
        }

        fn parse_lens_model(v: &DaedalusValue) -> Option<crate::modules::calibration::LensModel> {
            match v {
                DaedalusValue::Int(0) => Some(crate::modules::calibration::LensModel::Pinhole),
                DaedalusValue::Int(1) => Some(crate::modules::calibration::LensModel::Fisheye),
                DaedalusValue::Enum(DaedalusEnumValue { name, .. }) => match name.trim().to_ascii_lowercase().as_str() {
                    "pinhole" => Some(crate::modules::calibration::LensModel::Pinhole),
                    "fisheye" => Some(crate::modules::calibration::LensModel::Fisheye),
                    _ => None,
                },
                DaedalusValue::String(s) => match s.trim().to_ascii_lowercase().as_str() {
                    "pinhole" => Some(crate::modules::calibration::LensModel::Pinhole),
                    "fisheye" => Some(crate::modules::calibration::LensModel::Fisheye),
                    _ => None,
                },
                _ => None,
            }
        }

        fn parse_from_fields(fields: &[DaedalusStructFieldValue]) -> Option<crate::modules::aruco::detect::CameraCalibration> {
            let fx = get_field(fields, "fx").and_then(as_f32)?;
            let fy = get_field(fields, "fy").and_then(as_f32)?;
            let cx = get_field(fields, "cx").and_then(as_f32)?;
            let cy = get_field(fields, "cy").and_then(as_f32)?;
            let k1 = get_field(fields, "k1").and_then(as_f32)?;
            let k2 = get_field(fields, "k2").and_then(as_f32)?;
            let p1 = get_field(fields, "p1").and_then(as_f32)?;
            let p2 = get_field(fields, "p2").and_then(as_f32).unwrap_or(0.0);
            let k3 = get_field(fields, "k3").and_then(as_f32)?;
            let undistort_iters = get_field(fields, "undistortIters").and_then(as_u8).unwrap_or(5);
            let lens_model = get_field(fields, "lensModel").and_then(parse_lens_model).unwrap_or(crate::modules::calibration::LensModel::Pinhole);

            Some(crate::modules::aruco::detect::CameraCalibration { fx, fy, cx, cy, k1, k2, p1, p2, k3, undistort_iters, lens_model })
        }

        let parsed = match &calibration {
            DaedalusValue::Struct(fields) => parse_from_fields(fields),
            DaedalusValue::Map(entries) => {
                let mut tmp = Vec::with_capacity(entries.len());
                for (k, v) in entries {
                    let DaedalusValue::String(name) = k else { continue };
                    tmp.push(DaedalusStructFieldValue { name: name.to_string(), value: v.clone() });
                }
                parse_from_fields(&tmp)
            }
            _ => None,
        };
        let Some(calib) = parsed else {
            if !UNDISTORT_OPTIONAL_LOGGED_PARSE_FAIL.swap(true, std::sync::atomic::Ordering::Relaxed) {
                tracing::warn!(
                    target: "lib_cv::undistort_optional",
                    calibration_value = ?calibration,
                    "undistort_optional: calibration payload parse failed; passthrough frame"
                );
            }
            return Ok(frame);
        };
        if !UNDISTORT_OPTIONAL_LOGGED_OK.swap(true, std::sync::atomic::Ordering::Relaxed) {
            tracing::info!(
                target: "lib_cv::undistort_optional",
                fx = calib.fx,
                fy = calib.fy,
                cx = calib.cx,
                cy = calib.cy,
                k1 = calib.k1,
                k2 = calib.k2,
                p1 = calib.p1,
                p2 = calib.p2,
                k3 = calib.k3,
                lens_model = ?calib.lens_model,
                "undistort_optional: calibration parsed"
            );
        }
        calib
    };
    let fill_margin = fill_margin.clamp(1.0, 10.0) as f32;
    let zoom_manual = zoom.clamp(0.01, 100.0) as f32;
    let calib = UndistortCalib {
        in_fx: calibration.fx,
        in_fy: calibration.fy,
        in_cx: calibration.cx,
        in_cy: calibration.cy,
        out_fx: 1.0,
        out_fy: 1.0,
        out_cx: calibration.cx,
        out_cy: calibration.cy,
        k1: calibration.k1,
        k2: calibration.k2,
        p1: calibration.p1,
        p2: calibration.p2,
        k3: calibration.k3,
        lens_model: calibration.lens_model,
    };

    if !(calib.in_fx.is_finite() && calib.in_fy.is_finite() && calib.in_fx > 0.0 && calib.in_fy > 0.0) {
        return Ok(frame);
    }

    let img = expect_cpu(frame, "undistort_optional", Some(_exec_ctx))?;
    let (width, height) = img.dimensions();
    let zoom_eff = match zoom_mode {
        UndistortZoomMode::Manual => zoom_manual,
        UndistortZoomMode::Fill => {
            let tmp = calib;
            let z = compute_zoom_fill_cached(width, height, tmp);
            (z * fill_margin).max(0.01)
        }
    };
    let out_fx = calibration.fx * zoom_eff;
    let out_fy = calibration.fy * zoom_eff;
    if !(out_fx.is_finite() && out_fy.is_finite() && out_fx > 0.0 && out_fy > 0.0) {
        return Ok(Payload::Cpu(img));
    }
    let calib = UndistortCalib { out_fx, out_fy, ..calib };
    Ok(Payload::Cpu(undistort_dynamic_image(img, calib, border_clamp)))
}
