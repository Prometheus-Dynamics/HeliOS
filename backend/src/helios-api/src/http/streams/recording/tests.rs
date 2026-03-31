use super::media::append_suffix;

#[test]
fn append_suffix_preserves_double_extension() {
    assert_eq!(append_suffix("clip.tar.gz", "abc123", 1), "clip-abc123.tar.gz");
}

#[test]
fn append_suffix_preserves_regular_extension() {
    assert_eq!(append_suffix("clip.mp4", "abc123", 1), "clip-abc123.mp4");
    assert_eq!(append_suffix("clip.mp4", "abc123", 2), "clip-abc123-2.mp4");
}
