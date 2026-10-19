//! The look file that ships in the image (`gaia/assets/runtime-config/etc/lemnos/looks.d/helios.toml`)
//! checked against Lemnos's documented look keys (`docs/looks.md`), since Lemnos's parser is not a
//! dependency. Every look a state names must be defined, and every defined `helios.*` look must
//! be one a state names.

use super::states::State;

const LOOK_FILE: &str = include_str!("../../../../../../gaia/assets/runtime-config/etc/lemnos/looks.d/helios.toml");

/// The keys a block takes, with `block` itself, and every block takes `brightness` and `mode`.
fn block_keys(block: &str) -> Option<&'static [&'static str]> {
    Some(match block {
        "fill" => &["color"],
        "comet" => &["color", "period_ms", "tail", "heads", "base", "reverse"],
        "arc" => &["fraction", "color", "track", "head", "sheen"],
        "drain" => &["color", "fill_ms", "start_ms", "duration_ms", "easing"],
        "ripple" => &["color", "origin", "speed", "width", "settle_ms", "glow"],
        "frame" => &["pixels"],
        "wash" => &["color", "envelope"],
        "sparkle" => &["color", "colors", "density", "density_end", "fade_ms", "start_ms", "min_ms", "max_ms", "base", "seed", "fall", "fall_speed", "fall_accel"],
        "spots" => &["color", "spots", "width", "follow_ms", "max_spots"],
        _ => return None,
    })
}

fn is_colour(value: &toml::Value) -> bool {
    value.as_str().is_some_and(|text| text.len() == 6 && text.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Checks one look against the documented keys. The message names what is wrong.
fn check_look(name: &str, look: &toml::Table) -> Result<(), String> {
    for key in look.keys() {
        if !matches!(key.as_str(), "envelope" | "brightness" | "min_brightness" | "layers") {
            return Err(format!("{name}: unknown look key `{key}`"));
        }
    }
    let layers = look.get("layers").and_then(toml::Value::as_array).ok_or_else(|| format!("{name}: layers is a list"))?;
    if !(1..=4).contains(&layers.len()) {
        return Err(format!("{name}: a look has 1 to 4 layers, not {}", layers.len()));
    }
    let mut sparkles = 0;
    for layer in layers {
        let layer = layer.as_table().ok_or_else(|| format!("{name}: a layer is a table"))?;
        let block = layer.get("block").and_then(toml::Value::as_str).ok_or_else(|| format!("{name}: a layer names its block"))?;
        let keys = block_keys(block).ok_or_else(|| format!("{name}: unknown block `{block}`"))?;
        if block == "sparkle" {
            sparkles += 1;
        }
        for key in layer.keys() {
            let known = key == "block" || key == "brightness" || key == "mode" || keys.contains(&key.as_str());
            if !known {
                return Err(format!("{name}: `{block}` has no key `{key}`"));
            }
        }
        if let Some(mode) = layer.get("mode")
            && !matches!(mode.as_str(), Some("max" | "add" | "over"))
        {
            return Err(format!("{name}: a mode is max, add or over"));
        }
        if let Some(colour) = layer.get("color")
            && !is_colour(colour)
        {
            return Err(format!("{name}: `{colour}` is not a colour (rrggbb)"));
        }
        if let Some(pixels) = layer.get("pixels") {
            let count = pixels.as_array().map_or(0, Vec::len);
            if !(1..=64).contains(&count) || !pixels.as_array().is_some_and(|list| list.iter().all(is_colour)) {
                return Err(format!("{name}: a frame has 1 to 64 colours"));
            }
        }
        if let Some(fraction) = layer.get("fraction")
            && fraction.as_str() != Some("input")
            && !fraction.as_float().is_some_and(|value| (0.0..=1.0).contains(&value))
        {
            return Err(format!("{name}: an arc's fraction is \"input\" or 0 to 1"));
        }
        if let Some(origin) = layer.get("origin")
            && !origin.as_integer().is_some_and(|value| (0..=63).contains(&value))
        {
            return Err(format!("{name}: a ripple's origin is an LED, 0 to 63"));
        }
        if let Some(heads) = layer.get("heads")
            && !matches!(heads.as_integer(), Some(1 | 2))
        {
            return Err(format!("{name}: a comet has 1 or 2 heads"));
        }
        if let Some(envelope) = layer.get("envelope") {
            check_envelope(name, envelope)?;
        }
    }
    if sparkles > 1 {
        return Err(format!("{name}: a look has at most one sparkle"));
    }
    if let Some(envelope) = look.get("envelope") {
        check_envelope(name, envelope)?;
    }
    Ok(())
}

fn check_envelope(name: &str, envelope: &toml::Value) -> Result<(), String> {
    if envelope.as_str() == Some("solid") {
        return Ok(());
    }
    let table = envelope.as_table().ok_or_else(|| format!("{name}: an envelope is \"solid\" or a table"))?;
    match table.get("kind").and_then(toml::Value::as_str) {
        Some("breathe" | "blink" | "pulse") => Ok(()),
        _ => Err(format!("{name}: an envelope is breathe, blink or pulse")),
    }
}

fn look_names() -> Vec<String> {
    let table: toml::Table = LOOK_FILE.parse().expect("the look file is TOML");
    table.get("looks").and_then(toml::Value::as_table).expect("a [looks] table").keys().cloned().collect()
}

#[test]
fn the_look_file_matches_the_documented_keys() {
    let table: toml::Table = LOOK_FILE.parse().expect("the look file is TOML");
    let looks = table.get("looks").and_then(toml::Value::as_table).expect("a [looks] table");
    assert!(!looks.is_empty());
    for (name, look) in looks {
        assert!(name.starts_with("helios.") && name.len() <= 40, "a look name: {name}");
        assert!(name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_')), "{name}");
        let look = look.as_table().expect("a look is a table");
        if let Err(problem) = check_look(name, look) {
            panic!("{problem}");
        }
    }
}

#[test]
fn every_look_a_state_names_is_defined_and_every_look_is_named() {
    let defined = look_names();
    let mut named: Vec<&str> = Vec::new();
    for state in State::ALL {
        for look in [state.look(), state.tally_look()].into_iter().flatten() {
            if look == "status.off" {
                continue;
            }
            assert!(defined.iter().any(|name| name == look), "{look} is named by {state:?} but not defined");
            named.push(look);
        }
    }
    for look in ["helios.calibrating-view", "helios.rec.calibrating-view"] {
        assert!(defined.iter().any(|name| name == look), "{look} is defined");
        named.push(look);
    }
    for name in &defined {
        assert!(named.contains(&name.as_str()), "{name} is defined but no state uses it");
    }
}

#[test]
fn a_tally_variant_is_the_state_plus_one_frame_layer() {
    let table: toml::Table = LOOK_FILE.parse().expect("the look file is TOML");
    let looks = table.get("looks").and_then(toml::Value::as_table).expect("a [looks] table");
    for (name, value) in looks {
        let Some(base) = name.strip_prefix("helios.rec.") else { continue };
        let look = value.as_table().expect("a look is a table");
        let plain = looks.get(&format!("helios.{base}")).and_then(toml::Value::as_table).expect("the plain look exists");
        let layers = |look: &toml::Table| look.get("layers").and_then(toml::Value::as_array).map_or(0, Vec::len);
        let tally = look.get("layers").and_then(toml::Value::as_array).and_then(|layers| layers.last()).and_then(toml::Value::as_table);
        assert_eq!(layers(look), layers(plain) + 1, "{name} adds one layer");
        assert_eq!(tally.and_then(|layer| layer.get("block")).and_then(toml::Value::as_str), Some("frame"), "{name}: the tally is a frame");
    }
}

#[test]
fn the_checker_refuses_what_lemnos_refuses() {
    let bad = |text: &str| check_look("test", &text.parse::<toml::Table>().expect("toml")).is_err();
    assert!(bad(r#"layers = [{ block = "comet", color = "ff0000", heads = 3 }]"#));
    assert!(bad(r#"layers = [{ block = "fill", color = "zz0000" }]"#));
    assert!(bad(r#"layers = [{ block = "spin", color = "ff0000" }]"#));
    assert!(bad(r#"layers = [{ block = "fill", color = "ff0000", tail = 4 }]"#));
    assert!(bad(r#"envelope = "flash"
layers = [{ block = "fill", color = "ff0000" }]"#));
    assert!(bad(r#"layers = []"#));
    assert!(!bad(r#"layers = [{ block = "fill", color = "ff0000", brightness = 0.7 }]"#));
}
