use std::collections::BTreeSet;

use super::naming::{sanitize_table_name, unique_table_name};

#[test]
fn sanitize_table_name_collapses_noise() {
    assert_eq!(sanitize_table_name(" Front / Camera ", "fallback"), "front-camera");
}

#[test]
fn unique_table_name_suffixes_duplicates() {
    let mut seen = BTreeSet::new();
    assert_eq!(unique_table_name("limelight", &mut seen), "limelight");
    assert_eq!(unique_table_name("limelight", &mut seen), "limelight-2");
}
