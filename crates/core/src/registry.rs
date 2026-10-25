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
    let mac = cfg!(target_os = "macos");
    // HEIC/ICNS go through `sips`, PDF output through PDFKit: macOS only.
    let mac_only =
        |t: &&str| mac || !matches!(*t, "heic" | "icns") && (*t != "pdf" || pdf::available());
    let targets: Vec<&'static str> = match detect(path) {
        Kind::Image if image::can_decode(&e) => [
            "jpg", "png", "webp", "heic", "avif", "tiff", "gif", "bmp", "ico", "icns", "pdf",
        ]
        .into_iter()
        .filter(mac_only)
        .collect(),
        Kind::Svg => ["png", "jpg", "webp", "avif", "tiff", "pdf"]
            .into_iter()
            .filter(mac_only)
            .collect(),
        Kind::Audio => vec![
            "mp3", "m4a", "aac", "wav", "flac", "ogg", "opus", "aiff", "wma",
        ],
        Kind::Video => vec![
            "mp4", "mov", "m4v", "mkv", "webm", "avi", "wmv", "gif", "mp3", "m4a", "wav", "flac",
        ],
        Kind::Pdf if pdf::available() => {
            // Windows turns the PDF's text into documents with LibreOffice.
            let rich = mac || document::office_available();
            [
                "png", "jpg", "tiff", "docx", "odt", "rtf", "html", "txt", "md", "epub",
            ]
            .into_iter()
            .filter(|t| rich || matches!(*t, "png" | "jpg" | "tiff" | "txt"))
            .collect()
        }
        Kind::Document if document::can_read(&e) => document::targets(&e),
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
        Kind::Folder
        | Kind::Other
        | Kind::Image
        | Kind::Pdf
        | Kind::Document
        | Kind::Slides
        | Kind::Spreadsheet => vec!["zip", "tar.gz"],
    };
    targets
        .into_iter()
        .filter(|t| !same_format(t, &e))
        .collect()
}

/// Whether a file with extension `e` is already in the `target` format.
pub(crate) fn same_format(target: &str, e: &str) -> bool {
    match (target, e) {
        ("jpg", "jpeg") | ("tiff", "tif") | ("aiff", "aif") | ("heic", "heif") => true,
        ("html", "htm") | ("md", "markdown") | ("txt", "text") | ("ogg", "oga") => true,
        (t, e) => t == e,
    }
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
            ("fit-size", "Fit size"),
            ("resize-50", "Resize 50%"),
            ("resize-1080", "Fit 1080 px"),
            ("rotate-cw", "Rotate ↻"),
            ("rotate-ccw", "Rotate ↺"),
            ("flip-h", "Flip"),
            ("grayscale", "Grayscale"),
            ("strip-metadata", "Strip metadata"),
        ]);
        if count == 1 {
            tools.push(("crop", "Crop…"));
        }
        if vision::available() {
            tools.push(("ocr", "Extract text"));
        }
        if vision::can_cut_out() {
            tools.push(("remove-bg", "Remove BG"));
        }
        if count > 1 && pdf::available() {
            tools.push(("merge-pdf", "Combine PDF"));
        }
    } else if all(Kind::Video) {
        tools.extend([
            ("compress", "Compress"),
            ("fit-size", "Fit size"),
            ("resize-720", "720p"),
            ("resize-1080", "1080p"),
            ("extract-audio", "Extract audio"),
            ("mute", "Remove audio"),
            ("rotate-cw", "Rotate ↻"),
            ("speed-2x", "Speed ×2"),
            ("thumbnail", "Thumbnail"),
        ]);
        if count == 1 {
            tools.push(("trim", "Trim…"));
        }
        if count > 1 {
            tools.push(("merge", "Merge"));
        }
    } else if all(Kind::Audio) {
        tools.extend([
            ("compress", "Compress"),
            ("fit-size", "Fit size"),
            ("normalize", "Normalize"),
            ("mono", "Mono"),
            ("trim-silence", "Trim silence"),
        ]);
        if count == 1 {
            tools.push(("trim", "Trim…"));
        }
        if count > 1 {
            tools.push(("merge", "Merge"));
        }
    } else if all(Kind::Pdf) && pdf::available() {
        // Recompressing images needs PDFKit.
        if cfg!(target_os = "macos") {
            tools.push(("compress", "Compress"));
        }
        tools.extend([
            ("rotate-cw", "Rotate ↻"),
            ("split", "Split pages"),
            ("first-page", "First page"),
        ]);
        if vision::available() {
            tools.push(("ocr", "OCR text"));
        }
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
            .filter(|(id, _)| cfg!(target_os = "macos") || *id != "airdrop")
            .map(|(id, label)| Operation::new(category, format!("action:{id}"), *label))
            .collect(),
    }
}

/// Every convert and tool operation, grouped by the family of files it
/// applies to (`image`, `video`…), plus the headless actions under `any`.
/// Recipes are built from these.
pub fn catalog() -> Vec<(&'static str, Vec<Operation>)> {
    let families: &[(&str, &[&str])] = &[
        ("image", &["png", "jpg"]),
        ("video", &["mp4", "mov"]),
        ("audio", &["mp3", "wav"]),
        ("pdf", &["pdf"]),
        ("document", &["docx", "txt", "pptx"]),
        ("spreadsheet", &["xlsx", "csv"]),
        ("subtitle", &["srt", "vtt"]),
        ("archive", &["zip"]),
    ];
    let mut groups = Vec::new();
    for (family, exts) in families {
        let mut ops: Vec<Operation> = Vec::new();
        let mut add = |op: Operation| {
            if !ops.iter().any(|o| o.id == op.id) {
                ops.push(op);
            }
        };
        for e in *exts {
            let sample = PathBuf::from(format!("sample.{e}"));
            for t in convert_targets(&sample) {
                if !matches!(t, "zip" | "tar.gz") || *family == "archive" {
                    add(Operation::new(
                        Category::Convert,
                        format!("convert:{t}"),
                        label_for_target(t),
                    ));
                }
            }
            let kind = detect(&sample);
            for (id, label) in tool_list(&[kind, kind], 2) {
                // Trim and crop need a range picked in the editor.
                if !matches!(id, "archive" | "trim" | "crop") {
                    add(Operation::new(Category::Tool, format!("tool:{id}"), label));
                }
            }
        }
        if !ops.is_empty() {
            groups.push((*family, ops));
        }
    }
    groups.push((
        "any",
        vec![
            Operation::new(Category::Convert, "convert:zip", "ZIP"),
            Operation::new(Category::Tool, "tool:archive", "Zip together"),
            Operation::new(Category::Action, "action:copy", "Copy"),
            Operation::new(Category::Action, "action:copy-path", "Copy path"),
        ],
    ));
    groups
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
    // `trim=<start>-<end>` and `crop=<x>,<y>,<w>,<h>` carry their parameters.
    let (name, param) = match name.split_once('=') {
        Some((n, p)) => (n, Some(p)),
        None => (name, None),
    };
    let needs = |example: &str| {
        Error::Other(anyhow::anyhow!(
            "tool:{name} needs parameters, e.g. tool:{name}={example}"
        ))
    };
    let n = inputs.len() as f32;
    let mut out = Vec::new();
    for (i, input) in inputs.iter().enumerate() {
        let sub = |f: f32| progress((i as f32 + f) / n);
        let produced = match detect(input) {
            Kind::Audio | Kind::Video if name == "trim" => {
                let (start, end) = param
                    .and_then(parse_range)
                    .ok_or_else(|| needs("0:10-0:30"))?;
                media::trim(input, start, end, opts, &sub).map(|p| vec![p])
            }
            Kind::Image | Kind::Svg if name == "crop" => {
                let [x, y, w, h] = param
                    .and_then(parse_rect)
                    .ok_or_else(|| needs("0,0,800,600"))?;
                image::crop(input, x, y, w, h, opts).map(|p| vec![p])
            }
            Kind::Image if name == "ocr" => vision::image_text(input, opts).map(|p| vec![p]),
            Kind::Image if name == "fit-size" => image::fit_size(input, opts).map(|p| vec![p]),
            Kind::Audio | Kind::Video if name == "fit-size" => {
                media::fit_size(input, opts, &sub).map(|p| vec![p])
            }
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

/// `10-30`, `0:10-1:02.5` or `10-` (to the end), in seconds.
fn parse_range(s: &str) -> Option<(f64, Option<f64>)> {
    let (a, b) = s.split_once('-')?;
    let start = match a.trim() {
        "" => 0.0,
        a => media::parse_timestamp(a)?,
    };
    let end = match b.trim() {
        "" => None,
        b => Some(media::parse_timestamp(b)?),
    };
    Some((start, end))
}

fn parse_rect(s: &str) -> Option<[u32; 4]> {
    let v: Vec<u32> = s
        .split(',')
        .map(|p| p.trim().parse().ok())
        .collect::<Option<_>>()?;
    v.try_into().ok()
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
    fn parses_tool_parameters() {
        assert_eq!(parse_range("10-30"), Some((10.0, Some(30.0))));
        assert_eq!(parse_range("0:10-1:02.5"), Some((10.0, Some(62.5))));
        assert_eq!(parse_range("5-"), Some((5.0, None)));
        assert_eq!(parse_range("abc"), None);
        assert_eq!(parse_rect("1, 2,3,4"), Some([1, 2, 3, 4]));
        assert_eq!(parse_rect("1,2,3"), None);
        let err = run("tool:crop", &["a.png".into()], &Options::default(), &|_| {});
        assert!(err
            .unwrap_err()
            .to_string()
            .contains("tool:crop=0,0,800,600"));
    }

    #[test]
    fn catalog_groups_operations() {
        let groups = catalog();
        let image = &groups.iter().find(|(f, _)| *f == "image").unwrap().1;
        let image = ids(image.clone());
        assert!(image.contains(&"convert:jpg".to_string()));
        assert!(image.contains(&"convert:png".to_string()));
        assert!(image.contains(&"tool:fit-size".to_string()));
        assert!(!image.contains(&"tool:archive".to_string()));
        assert_eq!(groups.last().unwrap().0, "any");
    }

    #[test]
    fn multi_tools_need_several_files() {
        let one = ids(operations(&["a.mp3".into()], Category::Tool));
        let two = ids(operations(
            &["a.mp3".into(), "b.mp3".into()],
            Category::Tool,
        ));
        assert!(!one.contains(&"tool:merge".to_string()));
        assert!(two.contains(&"tool:merge".to_string()));
    }
}
