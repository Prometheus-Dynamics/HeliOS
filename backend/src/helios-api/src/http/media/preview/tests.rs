use super::metadata::{normalize_video_codec, parse_ffprobe_ratio};
use super::ranges::{MediaRange, parse_media_range};

#[test]
fn normalizes_known_video_codecs() {
    assert_eq!(normalize_video_codec("hevc").as_deref(), Some("h265"));
    assert_eq!(normalize_video_codec("avc1").as_deref(), Some("h264"));
    assert_eq!(normalize_video_codec("mjpeg").as_deref(), Some("mjpeg"));
    assert_eq!(normalize_video_codec("   ").as_deref(), None);
}

#[test]
fn parses_ffprobe_ratio_values() {
    assert_eq!(parse_ffprobe_ratio("30000/1001").map(|v| v as i32), Some(29));
    assert_eq!(parse_ffprobe_ratio("59.94").map(|v| v as i32), Some(59));
    assert_eq!(parse_ffprobe_ratio("1/0"), None);
    assert_eq!(parse_ffprobe_ratio(""), None);
}

#[test]
fn parses_byte_ranges() {
    assert_eq!(parse_media_range(Some("bytes=10-19"), 100), Ok(MediaRange::Partial { start: 10, end: 19 }));
    assert_eq!(parse_media_range(Some("bytes=10-"), 100), Ok(MediaRange::Partial { start: 10, end: 99 }));
    assert_eq!(parse_media_range(Some("bytes=-5"), 100), Ok(MediaRange::Partial { start: 95, end: 99 }));
    assert_eq!(parse_media_range(None, 100), Ok(MediaRange::Full));
    assert_eq!(parse_media_range(Some("bytes=100-101"), 100), Err(()));
}
