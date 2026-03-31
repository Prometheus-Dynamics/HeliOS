use super::ccm::patch_ccm_json;
use super::files::extract_ccm;

#[test]
fn patch_ccm_json_updates_matching_color_temperature() {
    let mut value = serde_json::json!({
        "algorithms": [{
            "rpi.ccm": {
                "ccms": [
                    {"ct": 3200, "ccm": [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]},
                    {"ct": 4000, "ccm": [0.9, 0.1, 0.0, 0.1, 0.9, 0.0, 0.0, 0.1, 0.9]}
                ]
            }
        }]
    });
    let replacement = [1.1, 0.2, 0.3, 0.4, 1.2, 0.5, 0.6, 0.7, 1.3];

    patch_ccm_json(&mut value, 4000, replacement).expect("patch succeeds");

    let ccms = value["algorithms"][0]["rpi.ccm"]["ccms"].as_array().expect("ccm array");
    let patched = ccms[1].as_object().expect("patched object");
    assert_eq!(patched.get("ct").and_then(|v| v.as_i64()), Some(4000));
    assert_eq!(patched.get("ccm"), Some(&serde_json::json!(replacement)));
}

#[test]
fn patch_ccm_json_falls_back_to_first_entry_when_temperature_missing() {
    let mut value = serde_json::json!({
        "algorithms": [{
            "rpi.ccm": {
                "ccms": [
                    {"ct": 3200, "ccm": [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]}
                ]
            }
        }]
    });
    let replacement = [0.8, 0.1, 0.2, 0.1, 0.8, 0.3, 0.2, 0.3, 0.8];

    patch_ccm_json(&mut value, 4500, replacement).expect("patch succeeds");

    let first = value["algorithms"][0]["rpi.ccm"]["ccms"][0].as_object().expect("patched object");
    assert_eq!(first.get("ct").and_then(|v| v.as_i64()), Some(4500));
    assert_eq!(first.get("ccm"), Some(&serde_json::json!(replacement)));
}

#[test]
fn extract_ccm_reads_first_matrix_and_color_temperature() {
    let bytes = br#"{
      "algorithms": [{
        "rpi.ccm": {
          "ccms": [{
            "ct": 3600,
            "ccm": [1.0, 0.1, 0.2, 0.3, 1.1, 0.4, 0.5, 0.6, 1.2]
          }]
        }
      }]
    }"#;

    let (ct, ccm) = extract_ccm(bytes).expect("ccm extracted");
    assert_eq!(ct, Some(3600));
    assert_eq!(ccm, Some([1.0, 0.1, 0.2, 0.3, 1.1, 0.4, 0.5, 0.6, 1.2]));
}

#[test]
fn extract_ccm_returns_none_for_invalid_matrix_length() {
    let bytes = br#"{
      "algorithms": [{
        "rpi.ccm": {
          "ccms": [{
            "ct": 3600,
            "ccm": [1.0, 0.1, 0.2]
          }]
        }
      }]
    }"#;

    assert!(extract_ccm(bytes).is_none());
}
