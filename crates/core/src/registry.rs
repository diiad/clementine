//! Which operations exist for a selection, and dispatching them to engines.

use crate::engines::{archive, document, image, media, pdf, sheet, subtitle, vision};
use crate::kind::{detect, ext, Kind};
use crate::{Error, Options, Progress, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Convert,
    Tool,
    Action,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct Operation {
    /// Stable identifier, e.g. `convert:webp`, `tool:compress`, `action:airdrop`.
    pub id: String,
    /// Short label shown on the wheel.
    pub label: String,
    pub category: Category,
}

impl Operation {
    fn new(category: Category, id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            category,
        }
    }
}

/// Output formats offered for a single input file.
fn convert_targets(path: &Path) -> Vec<&'static str> {
    let e = ext(path);
    let targets: Vec<&'static str> = match detect(path) {
        Kind::Image => vec![
            "jpg", "png", "webp", "heic", "avif", "tiff", "gif", "bmp", "ico", "icns", "pdf",
        ],
        Kind::Svg => vec!["png", "jpg", "webp", "avif", "tiff", "pdf"],
        Kind::Audio => vec![
            "mp3", "m4a", "aac", "wav", "flac", "ogg", "opus", "aiff", "wma",
        ],
        Kind::Video => vec![
            "mp4", "mov", "m4v", "mkv", "webm", "avi", "wmv", "gif", "mp3", "m4a", "wav", "flac",
        ],
        Kind::Pdf => vec![
            "png", "jpg", "tiff", "docx", "odt", "rtf", "html", "txt", "md", "epub",
        ],
        Kind::Document if document::can_read(&e) => {
            vec!["docx", "odt", "rtf", "pdf", "html", "txt", "md", "epub"]
        }
        Kind::Slides if document::office_available() => vec!["pdf", "pptx", "odp"],
        Kind::Spreadsheet if sheet::can_read(&e) => {
            let mut v = vec!["xlsx", "csv", "tsv", "json", "html", "md"];
            if document::office_available() {
                v.push("pdf");
            }
            v
        }
        Kind::Subtitle => vec!["srt", "vtt", "txt"],
        Kind::Archive => vec!["extract", "zip", "tar.gz"],
        Kind::Folder | Kind::Other | Kind::Document | Kind::Slides | Kind::Spreadsheet => {
            vec!["zip", "tar.gz"]
        }
    };
    let same = |t: &&str| match (*t, e.as_str()) {
        ("jpg", "jpeg") | ("tiff", "tif") | ("aiff", "aif") | ("heic", "heif") => true,
        ("html", "htm") | ("md", "markdown") | ("txt", "text") | ("ogg", "oga") => true,
        (t, e) => t == e,
    };
    targets.into_iter().filter(|t| !same(t)).collect()
}

fn label_for_target(t: &str) -> String {
    match t {
        "extract" => "Extract".into(),
        "tar.gz" => "TAR.GZ".into(),
        t => t.to_uppercase(),
    }
}

/// Tools for a selection. Tools marked `multi` combine several files into one.
fn tool_list(kinds: &[Kind], count: usize) -> Vec<(&'static str, &'static str)> {
    let all = |k: Kind| !kinds.is_empty() && kinds.iter().all(|x| *x == k);
    let mut tools = Vec::new();
    if all(Kind::Image) {
        tools.extend([
            ("compress", "Compress"),
            ("resize-50", "Resize 50%"),
            ("resize-1080", "Fit 1080 px"),
            ("rotate-cw", "Rotate ↻"),
            ("rotate-ccw", "Rotate ↺"),
            ("flip-h", "Flip"),
            ("grayscale", "Grayscale"),
            ("strip-metadata", "Strip metadata"),
            ("ocr", "Extract text"),
        ]);
        if vision::can_cut_out() {
            tools.push(("remove-bg", "Remove BG"));
        }
        if count > 1 {
            tools.push(("merge-pdf", "Combine PDF"));
        }
    } else if all(Kind::Video) {
        tools.extend([
            ("compress", "Compress"),
            ("resize-720", "720p"),
            ("resize-1080", "1080p"),
            ("extract-audio", "Extract audio"),
            ("mute", "Remove audio"),
            ("rotate-cw", "Rotate ↻"),
            ("speed-2x", "Speed ×2"),
            ("thumbnail", "Thumbnail"),
        ]);
        if count > 1 {
            tools.push(("merge", "Merge"));
        }
    } else if all(Kind::Audio) {
        tools.extend([
            ("compress", "Compress"),
            ("normalize", "Normalize"),
            ("mono", "Mono"),
            ("trim-silence", "Trim silence"),
        ]);
        if count > 1 {
            tools.push(("merge", "Merge"));
        }
    } else if all(Kind::Pdf) {
        tools.extend([
            ("compress", "Compress"),
            ("rotate-cw", "Rotate ↻"),
            ("split", "Split pages"),
            ("first-page", "First page"),
            ("ocr", "OCR text"),
        ]);
        if count > 1 {
            tools.push(("merge", "Merge"));
        }
    } else if all(Kind::Subtitle) {
        tools.extend([("shift+1", "Delay +1 s"), ("shift-1", "Advance −1 s")]);
    } else if all(Kind::Spreadsheet) && count > 1 {
        tools.push(("merge", "Merge sheets"));
    }
    if count > 1 {
        tools.push(("archive", "Zip together"));
    }
    tools
}

const ACTIONS: &[(&str, &str)] = &[
    ("airdrop", "AirDrop"),
    ("float", "Float"),
    ("copy", "Copy"),
    ("move", "Move…"),
    ("copy-path", "Copy path"),
    ("terminal", "Terminal"),
];

/// Everything that can be done with `inputs` in the given category.
pub fn operations(inputs: &[PathBuf], category: Category) -> Vec<Operation> {
    if inputs.is_empty() {
        return Vec::new();
    }
    match category {
        Category::Convert => {
            // Only formats every selected file can be converted to.
            let mut common = convert_targets(&inputs[0]);
            for p in &inputs[1..] {
                let t = convert_targets(p);
                common.retain(|x| t.contains(x));
            }
            common
                .into_iter()
                .map(|t| Operation::new(category, format!("convert:{t}"), label_for_target(t)))
                .collect()
        }
        Category::Tool => {
            let kinds: Vec<Kind> = inputs.iter().map(|p| detect(p)).collect();
            tool_list(&kinds, inputs.len())
                .into_iter()
                .map(|(id, label)| Operation::new(category, format!("tool:{id}"), label))
                .collect()
        }
        Category::Action => ACTIONS
            .iter()
            .map(|(id, label)| Operation::new(category, format!("action:{id}"), *label))
            .collect(),
    }
}

/// Execute `op_id` on `inputs` and return the files that were produced.
///
/// Actions (`action:*`) are platform/UI concerns and are executed by the app,
/// except for the few that make sense headless (see [`crate::engines::actions`]).
pub fn run(
    op_id: &str,
    inputs: &[PathBuf],
    opts: &Options,
    progress: Progress,
) -> Result<Vec<PathBuf>> {
    let (cat, name) = op_id
        .split_once(':')
        .ok_or_else(|| Error::UnknownOperation(op_id.into()))?;
    match cat {
        "convert" => {
            let mut out = Vec::new();
            let n = inputs.len() as f32;
            for (i, input) in inputs.iter().enumerate() {
                let sub = |f: f32| progress((i as f32 + f) / n);
                out.extend(convert_one(input, name, opts, &sub)?);
            }
            progress(1.0);
            Ok(out)
        }
        "tool" => run_tool(name, inputs, opts, progress),
        "action" => crate::engines::actions::run(name, inputs, opts),
        _ => Err(Error::UnknownOperation(op_id.into())),
    }
}

fn unsupported(op: &str, input: &Path) -> Error {
    Error::Unsupported {
        op: op.into(),
        file: input.display().to_string(),
    }
}

fn convert_one(
    input: &Path,
    target: &str,
    opts: &Options,
    progress: Progress,
) -> Result<Vec<PathBuf>> {
    if !convert_targets(input).contains(&target) {
        return Err(unsupported(&format!("convert:{target}"), input));
    }
    match (detect(input), target) {
        (_, "zip") | (_, "tar.gz") if detect(input) != Kind::Archive => {
            archive::create(&[input.to_path_buf()], target, opts).map(|p| vec![p])
        }
        (Kind::Archive, "extract") => archive::extract(input, opts).map(|p| vec![p]),
        (Kind::Archive, t) => archive::repack(input, t, opts).map(|p| vec![p]),
        (Kind::Image | Kind::Svg, "pdf") => {
            pdf::images_to_pdf(&[input.to_path_buf()], opts).map(|p| vec![p])
        }
        (Kind::Image | Kind::Svg, t) => image::convert(input, t, opts).map(|p| vec![p]),
        (Kind::Audio | Kind::Video, t) => media::convert(input, t, opts, progress).map(|p| vec![p]),
        (Kind::Pdf, "png" | "jpg" | "tiff") => pdf::to_images(input, target, opts, progress),
        (Kind::Pdf, t) => document::from_pdf(input, t, opts).map(|p| vec![p]),
        (Kind::Document | Kind::Slides, t) => document::convert(input, t, opts).map(|p| vec![p]),
        (Kind::Spreadsheet, "pdf") => document::office_convert(input, "pdf", opts).map(|p| vec![p]),
        (Kind::Spreadsheet, t) => sheet::convert(input, t, opts),
        (Kind::Subtitle, t) => subtitle::convert(input, t, opts).map(|p| vec![p]),
        _ => Err(unsupported(&format!("convert:{target}"), input)),
    }
}

fn run_tool(
    name: &str,
    inputs: &[PathBuf],
    opts: &Options,
    progress: Progress,
) -> Result<Vec<PathBuf>> {
    let first = inputs
        .first()
        .ok_or_else(|| Error::UnknownOperation(name.into()))?;
    if name == "archive" {
        return archive::create(inputs, "zip", opts).map(|p| vec![p]);
    }
    let kind = detect(first);
    let combine = matches!(name, "merge" | "merge-pdf");
    if combine {
        return match kind {
            Kind::Image => pdf::images_to_pdf(inputs, opts).map(|p| vec![p]),
            Kind::Pdf => pdf::merge(inputs, opts).map(|p| vec![p]),
            Kind::Audio | Kind::Video => media::concat(inputs, opts, progress).map(|p| vec![p]),
            Kind::Spreadsheet => sheet::merge(inputs, opts).map(|p| vec![p]),
            _ => Err(unsupported(name, first)),
        };
    }
    let n = inputs.len() as f32;
    let mut out = Vec::new();
    for (i, input) in inputs.iter().enumerate() {
        let sub = |f: f32| progress((i as f32 + f) / n);
        let produced = match detect(input) {
            Kind::Image if name == "ocr" => vision::image_text(input, opts).map(|p| vec![p]),
            Kind::Image if name == "remove-bg" => {
                vision::remove_background(input, opts).map(|p| vec![p])
            }
            Kind::Image => image::tool(input, name, opts).map(|p| vec![p]),
            Kind::Pdf if name == "ocr" => pdf::ocr(input, opts, &sub).map(|p| vec![p]),
            Kind::Audio | Kind::Video => media::tool(input, name, opts, &sub).map(|p| vec![p]),
            Kind::Pdf => pdf::tool(input, name, opts),
            Kind::Subtitle => subtitle::tool(input, name, opts).map(|p| vec![p]),
            _ => Err(unsupported(name, input)),
        }?;
        out.extend(produced);
    }
    progress(1.0);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(ops: Vec<Operation>) -> Vec<String> {
        ops.into_iter().map(|o| o.id).collect()
    }

    #[test]
    fn convert_targets_exclude_source_format() {
        let ops = ids(operations(&["a.jpeg".into()], Category::Convert));
        assert!(!ops.contains(&"convert:jpg".to_string()));
        assert!(ops.contains(&"convert:png".to_string()));
    }

    #[test]
    fn mixed_selection_keeps_common_targets() {
        let ops = ids(operations(
            &["a.png".into(), "b.jpg".into()],
            Category::Convert,
        ));
        assert!(ops.contains(&"convert:webp".to_string()));
        assert!(!ops.contains(&"convert:png".to_string()));
        assert!(!ops.contains(&"convert:jpg".to_string()));
    }

    #[test]
    fn multi_tools_need_several_files() {
        let one = ids(operations(&["a.pdf".into()], Category::Tool));
        let two = ids(operations(
            &["a.pdf".into(), "b.pdf".into()],
            Category::Tool,
        ));
        assert!(!one.contains(&"tool:merge".to_string()));
        assert!(two.contains(&"tool:merge".to_string()));
    }
}
