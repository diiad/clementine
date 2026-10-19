//! On-device machine learning through Apple's Vision framework: text
//! recognition (OCR) and subject cut-out. Nothing leaves the Mac.

use super::{image as img, scratch};
use crate::{output_path, Error, Options, Result};
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use objc2::rc::Retained;
    use objc2::runtime::AnyClass;
    use objc2::AllocAnyThread;
    use objc2_core_video::{
        kCVPixelFormatType_OneComponent32Float, CVPixelBufferGetBaseAddress,
        CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType,
        CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
        CVPixelBufferUnlockBaseAddress,
    };
    use objc2_foundation::{NSArray, NSDictionary, NSError, NSString, NSURL};
    use objc2_vision::{
        VNGenerateForegroundInstanceMaskRequest, VNImageRequestHandler, VNRecognizeTextRequest,
        VNRequest, VNRequestTextRecognitionLevel,
    };

    fn ns_error(e: Retained<NSError>) -> Error {
        anyhow::anyhow!("Vision: {}", e.localizedDescription()).into()
    }

    fn handler(path: &Path) -> Retained<VNImageRequestHandler> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        unsafe {
            VNImageRequestHandler::initWithURL_options(
                VNImageRequestHandler::alloc(),
                &url,
                &NSDictionary::new(),
            )
        }
    }

    fn perform(handler: &VNImageRequestHandler, request: &VNRequest) -> Result<()> {
        handler
            .performRequests_error(&NSArray::from_slice(&[request]))
            .map_err(ns_error)
    }

    pub fn recognize_text(path: &Path) -> Result<String> {
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(true);
        request.setAutomaticallyDetectsLanguage(true);
        perform(&handler(path), &request)?;
        let mut boxes = Vec::new();
        if let Some(results) = request.results() {
            for observation in results.iter() {
                if let Some(best) = observation.topCandidates(1).firstObject() {
                    let r = unsafe { observation.boundingBox() };
                    boxes.push(TextBox {
                        x: r.origin.x,
                        // Vision's origin is bottom-left; flip to top-down.
                        y: 1.0 - (r.origin.y + r.size.height / 2.0),
                        h: r.size.height,
                        text: best.string().to_string(),
                    });
                }
            }
        }
        Ok(reading_order(boxes))
    }

    pub fn can_cut_out() -> bool {
        // macOS 14+.
        AnyClass::get(c"VNGenerateForegroundInstanceMaskRequest").is_some()
    }

    /// Foreground mask at the image's resolution (0 = background, 255 =
    /// subject), or `None` when Vision finds no subject.
    pub fn foreground_mask(path: &Path) -> Result<Option<(usize, usize, Vec<u8>)>> {
        let handler = handler(path);
        let request = unsafe { VNGenerateForegroundInstanceMaskRequest::new() };
        perform(&handler, &request)?;
        let Some(observation) = unsafe { request.results() }.and_then(|r| r.firstObject()) else {
            return Ok(None);
        };
        let buffer = unsafe {
            observation.generateScaledMaskForImageForInstances_fromRequestHandler_error(
                &observation.allInstances(),
                &handler,
            )
        }
        .map_err(ns_error)?;
        if CVPixelBufferGetPixelFormatType(&buffer) != kCVPixelFormatType_OneComponent32Float {
            return Err(anyhow::anyhow!("unexpected mask format").into());
        }
        let ro = CVPixelBufferLockFlags::ReadOnly;
        unsafe { CVPixelBufferLockBaseAddress(&buffer, ro) };
        let (w, h) = (
            CVPixelBufferGetWidth(&buffer),
            CVPixelBufferGetHeight(&buffer),
        );
        let stride = CVPixelBufferGetBytesPerRow(&buffer);
        let base = CVPixelBufferGetBaseAddress(&buffer) as *const u8;
        let mut mask = Vec::with_capacity(w * h);
        for y in 0..h {
            // SAFETY: the buffer is locked and holds `h` rows of `stride`
            // bytes, each starting with `w` f32 values.
            let row = unsafe { std::slice::from_raw_parts(base.add(y * stride).cast::<f32>(), w) };
            mask.extend(
                row.iter()
                    .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8),
            );
        }
        unsafe { CVPixelBufferUnlockBaseAddress(&buffer, ro) };
        Ok(Some((w, h, mask)))
    }
}

#[cfg(not(target_os = "macos"))]
mod mac {
    use super::*;
    fn unsupported<T>() -> Result<T> {
        Err(anyhow::anyhow!("Vision is only available on macOS").into())
    }
    pub fn recognize_text(_: &Path) -> Result<String> {
        unsupported()
    }
    pub fn can_cut_out() -> bool {
        false
    }
    pub fn foreground_mask(_: &Path) -> Result<Option<(usize, usize, Vec<u8>)>> {
        unsupported()
    }
}

pub use mac::can_cut_out;

/// A recognised run of text, in normalised image coordinates (top-down).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
struct TextBox {
    x: f64,
    /// Vertical centre.
    y: f64,
    h: f64,
    text: String,
}

/// Vision reports text runs column by column. Rebuild lines: runs whose
/// centres sit within half a line height of each other share a line, left
/// to right; a gap taller than a line starts a new paragraph.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn reading_order(mut boxes: Vec<TextBox>) -> String {
    boxes.sort_by(|a, b| a.y.total_cmp(&b.y));
    let mut lines: Vec<Vec<TextBox>> = Vec::new();
    for b in boxes {
        match lines.last_mut() {
            Some(line) if (b.y - line[0].y).abs() < line[0].h.max(b.h) / 2.0 => line.push(b),
            _ => lines.push(vec![b]),
        }
    }
    let mut out = String::new();
    let mut prev: Option<(f64, f64)> = None;
    for mut line in lines {
        line.sort_by(|a, b| a.x.total_cmp(&b.x));
        let (y, h) = (line[0].y, line[0].h);
        if let Some((py, ph)) = prev {
            out.push('\n');
            if y - py > 1.8 * h.max(ph) {
                out.push('\n');
            }
        }
        let words: Vec<&str> = line.iter().map(|b| b.text.as_str()).collect();
        out.push_str(&words.join("  "));
        prev = Some((y, h));
    }
    out
}

/// Recognise the text in an image file Vision can read (PNG, JPEG…).
pub fn recognize_text(path: &Path) -> Result<String> {
    mac::recognize_text(path)
}

/// Decode with our own pipeline (EXIF orientation, SVG, HEIC) and hand
/// Vision a plain PNG, so that results line up with what the user sees.
fn staged(
    input: &Path,
    opts: &Options,
) -> Result<(tempfile::TempDir, PathBuf, image::DynamicImage)> {
    let decoded = img::decode(input, opts.dpi)?;
    let tmp = scratch()?;
    let png = tmp.path().join("vision.png");
    img::encode(&decoded, "png", &png, 100)?;
    Ok((tmp, png, decoded))
}

/// Image → `.txt` with the text it contains.
pub fn image_text(input: &Path, opts: &Options) -> Result<PathBuf> {
    let (_tmp, png, _) = staged(input, opts)?;
    let text = recognize_text(&png)?;
    if text.trim().is_empty() {
        return Err(anyhow::anyhow!("no text found").into());
    }
    let out = output_path(input, "", "txt", opts);
    std::fs::write(&out, text + "\n")?;
    Ok(out)
}

/// Image → transparent PNG of its main subject.
pub fn remove_background(input: &Path, opts: &Options) -> Result<PathBuf> {
    if !can_cut_out() {
        return Err(Error::Unsupported {
            op: "tool:remove-bg (needs macOS 14)".into(),
            file: input.display().to_string(),
        });
    }
    let (_tmp, png, decoded) = staged(input, opts)?;
    let (w, h, mask) =
        mac::foreground_mask(&png)?.ok_or_else(|| anyhow::anyhow!("no subject found"))?;
    let mut rgba = decoded.to_rgba8();
    if (w as u32, h as u32) != rgba.dimensions() {
        return Err(anyhow::anyhow!("mask size {w}×{h} does not match the image").into());
    }
    for (px, m) in rgba.pixels_mut().zip(mask) {
        px.0[3] = px.0[3].min(m);
    }
    let out = output_path(input, "-cutout", "png", opts);
    img::encode(
        &image::DynamicImage::ImageRgba8(rgba),
        "png",
        &out,
        opts.quality,
    )?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(x: f64, y: f64, text: &str) -> TextBox {
        TextBox {
            x,
            y,
            h: 0.02,
            text: text.into(),
        }
    }

    #[test]
    fn rebuilds_lines_and_paragraphs() {
        let text = reading_order(vec![
            b(0.5, 0.101, "B"),
            b(0.1, 0.2, "next paragraph"),
            b(0.1, 0.1, "A"),
            b(0.1, 0.125, "same para"),
        ]);
        assert_eq!(text, "A  B\nsame para\n\nnext paragraph");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reads_rendered_text() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("scan.md");
        std::fs::write(&md, "# Bonjour Clémentine\n\nLe texte reconnu.\n").unwrap();
        let opts = Options::default();
        let pdf = super::super::document::convert(&md, "pdf", &opts).unwrap();
        let png = super::super::pdf::to_images(&pdf, "png", &opts, &|_| {}).unwrap();
        let txt = image_text(&png[0], &opts).unwrap();
        let text = std::fs::read_to_string(txt).unwrap();
        assert!(
            text.contains("Bonjour") && text.contains("reconnu"),
            "{text}"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cuts_out_a_subject() {
        let photo = Path::new("/Library/User Pictures/Animals/Penguin.heic");
        if !photo.exists() || !can_cut_out() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let opts = Options {
            out_dir: Some(dir.path().to_path_buf()),
            ..Options::default()
        };
        let out = remove_background(photo, &opts).unwrap();
        let rgba = image::open(out).unwrap().to_rgba8();
        assert_eq!(rgba.get_pixel(5, 5).0[3], 0, "corner should be transparent");
        let (w, h) = rgba.dimensions();
        assert!(rgba.get_pixel(w / 2, h * 3 / 4).0[3] > 200, "subject kept");
    }
}
