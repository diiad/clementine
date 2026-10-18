//! File-kind detection, based on the file extension.

use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Image,
    Svg,
    Audio,
    Video,
    Pdf,
    Document,
    Slides,
    Spreadsheet,
    Subtitle,
    Archive,
    Folder,
    Other,
}

pub const IMAGE_IN: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "heic", "heif", "tiff", "tif", "avif", "bmp", "gif", "ico", "tga",
];
pub const AUDIO_IN: &[&str] = &[
    "mp3", "m4a", "aac", "wav", "flac", "ogg", "oga", "opus", "aiff", "aif", "wma", "caf", "alac",
];
pub const VIDEO_IN: &[&str] = &[
    "mp4", "mov", "m4v", "mkv", "webm", "avi", "wmv", "flv", "mpg", "mpeg", "3gp", "ts", "mts",
];
pub const DOCUMENT_IN: &[&str] = &[
    "docx", "doc", "rtf", "odt", "txt", "text", "html", "htm", "md", "markdown", "epub",
    "webarchive", "pages",
];
pub const SLIDES_IN: &[&str] = &["pptx", "ppt", "odp", "key"];
pub const SPREADSHEET_IN: &[&str] = &["xlsx", "xlsm", "xls", "xlsb", "ods", "csv", "tsv", "numbers"];
pub const SUBTITLE_IN: &[&str] = &["srt", "vtt"];
pub const ARCHIVE_IN: &[&str] = &[
    "zip", "tar", "tgz", "gz", "bz2", "tbz", "xz", "txz", "rar", "7z", "cpio", "iso",
];

/// Lower-cased extension of `path`, with `.tar.gz`-style double extensions folded.
pub fn ext(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    for double in ["tar.gz", "tar.bz2", "tar.xz"] {
        if name.ends_with(&format!(".{double}")) {
            return double.to_string();
        }
    }
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

pub fn detect(path: &Path) -> Kind {
    if path.is_dir() {
        return Kind::Folder;
    }
    kind_of_ext(&ext(path))
}

pub fn kind_of_ext(e: &str) -> Kind {
    let e = e.trim_start_matches('.');
    if e == "svg" {
        Kind::Svg
    } else if e == "pdf" {
        Kind::Pdf
    } else if IMAGE_IN.contains(&e) {
        Kind::Image
    } else if AUDIO_IN.contains(&e) {
        Kind::Audio
    } else if VIDEO_IN.contains(&e) {
        Kind::Video
    } else if DOCUMENT_IN.contains(&e) {
        Kind::Document
    } else if SLIDES_IN.contains(&e) {
        Kind::Slides
    } else if SPREADSHEET_IN.contains(&e) {
        Kind::Spreadsheet
    } else if SUBTITLE_IN.contains(&e) {
        Kind::Subtitle
    } else if ARCHIVE_IN.contains(&e) || e.starts_with("tar.") {
        Kind::Archive
    } else {
        Kind::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_kinds() {
        assert_eq!(kind_of_ext("JPG".to_lowercase().as_str()), Kind::Image);
        assert_eq!(kind_of_ext("mkv"), Kind::Video);
        assert_eq!(kind_of_ext("srt"), Kind::Subtitle);
        assert_eq!(ext(Path::new("/a/b.TAR.GZ")), "tar.gz");
        assert_eq!(detect(Path::new("x.tar.gz")), Kind::Archive);
        assert_eq!(detect(Path::new("x.unknown")), Kind::Other);
    }
}
