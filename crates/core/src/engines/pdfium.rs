//! PDF on Windows through PDFium, the engine Chrome uses. `pdfium.dll` ships
//! next to the app (or is found through `CLEMENTINE_PDFIUM` / the system).

use crate::engines::image as img;
use crate::kind::{ext, kind_of_ext, Kind};
use crate::{output_path, Error, Options, Progress, Result};
use pdfium_render::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static PDFIUM: OnceLock<std::result::Result<Pdfium, String>> = OnceLock::new();

fn bind() -> std::result::Result<Pdfium, String> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("CLEMENTINE_PDFIUM") {
        candidates.push(PathBuf::from(p));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        candidates.push(Pdfium::pdfium_platform_library_name_at_path(&dir));
        candidates.push(Pdfium::pdfium_platform_library_name_at_path(
            &dir.join("resources"),
        ));
    }
    for path in candidates.iter().filter(|p| p.is_file()) {
        if let Ok(bindings) = Pdfium::bind_to_library(path) {
            return Ok(Pdfium::new(bindings));
        }
    }
    Pdfium::bind_to_system_library()
        .map(Pdfium::new)
        .map_err(|e| format!("PDFium (pdfium.dll) not found: {e}"))
}

fn pdfium() -> Result<&'static Pdfium> {
    PDFIUM
        .get_or_init(bind)
        .as_ref()
        .map_err(|e| Error::Other(anyhow::anyhow!("{e}")))
}

fn err(e: PdfiumError) -> Error {
    Error::Other(anyhow::anyhow!("PDFium: {e}"))
}

fn open(path: &Path) -> Result<PdfDocument<'static>> {
    pdfium()?.load_pdf_from_file(path, None).map_err(err)
}

fn save(doc: &PdfDocument, out: &Path) -> Result<()> {
    doc.save_to_file(out).map_err(err)
}

fn unsupported(op: &str, input: &Path) -> Error {
    Error::Unsupported {
        op: op.into(),
        file: input.display().to_string(),
    }
}

pub fn available() -> bool {
    pdfium().is_ok()
}

pub fn page_count(input: &Path) -> Result<usize> {
    Ok(open(input)?.pages().len() as usize)
}

/// One image per page: `doc.png` for single pages, `doc-1.png`, `doc-2.png`… otherwise.
pub fn to_images(
    input: &Path,
    target: &str,
    opts: &Options,
    progress: Progress,
) -> Result<Vec<PathBuf>> {
    let doc = open(input)?;
    let n = doc.pages().len() as usize;
    let mut out = Vec::with_capacity(n);
    for (i, page) in doc.pages().iter().enumerate() {
        // Points are 1/72 in.
        let width = (page.width().value / 72.0 * opts.dpi as f32)
            .round()
            .max(1.0) as i32;
        let config = PdfRenderConfig::new().set_target_width(width);
        let image = page
            .render_with_config(&config)
            .map_err(err)?
            .as_image()
            .map_err(err)?;
        let suffix = if n == 1 {
            String::new()
        } else {
            format!("-{}", i + 1)
        };
        let path = output_path(input, &suffix, target, opts);
        img::encode(&image, target, &path, opts.quality)?;
        out.push(path);
        progress((i + 1) as f32 / n as f32);
    }
    Ok(out)
}

/// Scanned PDF → `.txt`: each page rendered and read by the system OCR.
pub fn ocr(input: &Path, opts: &Options, progress: Progress) -> Result<PathBuf> {
    if !crate::engines::vision::available() {
        return Err(unsupported("tool:ocr", input));
    }
    let doc = open(input)?;
    let n = doc.pages().len() as usize;
    let tmp = crate::engines::scratch()?;
    let png = tmp.path().join("page.png");
    let mut pages = Vec::with_capacity(n);
    for (i, page) in doc.pages().iter().enumerate() {
        // 200 dpi is plenty for OCR and keeps big scans fast.
        let width = (page.width().value / 72.0 * opts.dpi.min(200) as f32)
            .round()
            .max(1.0) as i32;
        let image = page
            .render_with_config(&PdfRenderConfig::new().set_target_width(width))
            .map_err(err)?
            .as_image()
            .map_err(err)?;
        img::encode(&image, "png", &png, 100)?;
        pages.push(crate::engines::vision::recognize_text(&png)?);
        progress((i + 1) as f32 / n as f32);
    }
    if pages.iter().all(|p| p.trim().is_empty()) {
        return Err(anyhow::anyhow!("no text found").into());
    }
    let out = output_path(input, "", "txt", opts);
    std::fs::write(&out, pages.join("\n\n") + "\n")?;
    Ok(out)
}

pub fn text(input: &Path) -> Result<String> {
    let doc = open(input)?;
    let mut pages = Vec::new();
    for page in doc.pages().iter() {
        pages.push(page.text().map_err(err)?.all());
    }
    Ok(pages.join("\n\n"))
}

/// The text as a plain RTF document, for the PDF → documents path, which
/// hands RTF to LibreOffice.
pub fn rtf(input: &Path) -> Result<Vec<u8>> {
    let mut out = String::from("{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0 Calibri;}}\\f0\\fs22\n");
    for ch in text(input)?.chars() {
        match ch {
            '\\' | '{' | '}' => {
                out.push('\\');
                out.push(ch);
            }
            '\n' => out.push_str("\\par\n"),
            '\r' => {}
            c if c.is_ascii() => out.push(c),
            // RTF \uN takes a signed 16-bit value, plus a fallback character.
            c => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{}?", *unit as i16));
                }
            }
        }
    }
    out.push('}');
    Ok(out.into_bytes())
}

/// Only the macOS engine lays out RTF itself; Windows prints documents
/// through LibreOffice.
pub fn from_rtf(_rtf: &[u8], out: &Path) -> Result<()> {
    Err(unsupported("convert:pdf", out))
}

pub fn merge(inputs: &[PathBuf], opts: &Options) -> Result<PathBuf> {
    let mut merged = pdfium()?.create_new_pdf().map_err(err)?;
    for input in inputs {
        let doc = open(input)?;
        merged.pages_mut().append(&doc).map_err(err)?;
    }
    let out = output_path(&inputs[0], "-merged", "pdf", opts);
    save(&merged, &out)?;
    Ok(out)
}

/// Combine images into one PDF, one page each, sized like the image (1 px = 1 pt).
pub fn images_to_pdf(inputs: &[PathBuf], opts: &Options) -> Result<PathBuf> {
    let mut doc = pdfium()?.create_new_pdf().map_err(err)?;
    for input in inputs {
        let dpi = if kind_of_ext(&ext(input)) == Kind::Svg {
            opts.dpi.min(192)
        } else {
            opts.dpi
        };
        let image = img::decode(input, dpi)?;
        let (w, h) = (
            PdfPoints::new(image.width() as f32),
            PdfPoints::new(image.height() as f32),
        );
        let mut page = doc
            .pages_mut()
            .create_page_at_end(PdfPagePaperSize::Custom(w, h))
            .map_err(err)?;
        page.objects_mut()
            .create_image_object(PdfPoints::ZERO, PdfPoints::ZERO, &image, Some(w), Some(h))
            .map_err(err)?;
    }
    let suffix = if inputs.len() > 1 { "-combined" } else { "" };
    let out = output_path(&inputs[0], suffix, "pdf", opts);
    save(&doc, &out)?;
    Ok(out)
}

fn single_page(doc: &PdfDocument, index: i32) -> Result<PdfDocument<'static>> {
    let mut single = pdfium()?.create_new_pdf().map_err(err)?;
    single
        .pages_mut()
        .copy_page_range_from_document(doc, index..=index, 0)
        .map_err(err)?;
    Ok(single)
}

pub fn tool(input: &Path, name: &str, opts: &Options) -> Result<Vec<PathBuf>> {
    let doc = open(input)?;
    let n = doc.pages().len();
    match name {
        "rotate-cw" => {
            for i in 0..n {
                let mut page = doc.pages().get(i).map_err(err)?;
                let next = match page.rotation().map_err(err)? {
                    PdfPageRenderRotation::None => PdfPageRenderRotation::Degrees90,
                    PdfPageRenderRotation::Degrees90 => PdfPageRenderRotation::Degrees180,
                    PdfPageRenderRotation::Degrees180 => PdfPageRenderRotation::Degrees270,
                    PdfPageRenderRotation::Degrees270 => PdfPageRenderRotation::None,
                };
                page.set_rotation(next);
            }
            let out = output_path(input, "-rotated", "pdf", opts);
            save(&doc, &out)?;
            Ok(vec![out])
        }
        "split" => {
            let mut out = Vec::with_capacity(n as usize);
            for i in 0..n {
                let path = output_path(input, &format!("-p{}", i + 1), "pdf", opts);
                save(&single_page(&doc, i)?, &path)?;
                out.push(path);
            }
            Ok(out)
        }
        "first-page" => {
            let path = output_path(input, "-p1", "pdf", opts);
            save(&single_page(&doc, 0)?, &path)?;
            Ok(vec![path])
        }
        _ => Err(unsupported(&format!("tool:{name}"), input)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-page PDF with a line of text. PDFium rebuilds the missing xref.
    const HELLO: &[u8] = b"%PDF-1.4
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 4 0 R
  /Resources << /Font << /F1 5 0 R >> >> >> endobj
4 0 obj << /Length 44 >> stream
BT /F1 24 Tf 40 100 Td (Bonjour PDF) Tj ET
endstream endobj
5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj
trailer << /Root 1 0 R >>
%%EOF
";

    #[test]
    fn pdf_operations() {
        if !available() {
            eprintln!("PDFium not found, skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let opts = Options::default();
        let hello = dir.path().join("hello.pdf");
        std::fs::write(&hello, HELLO).unwrap();
        assert_eq!(page_count(&hello).unwrap(), 1);
        assert!(text(&hello).unwrap().contains("Bonjour PDF"));
        assert!(String::from_utf8(rtf(&hello).unwrap())
            .unwrap()
            .contains("Bonjour PDF"));

        // 300 pt wide at 144 dpi → 600 px.
        let png = to_images(
            &hello,
            "png",
            &Options {
                dpi: 144,
                ..opts.clone()
            },
            &|_| {},
        )
        .unwrap();
        let rendered = image::open(&png[0]).unwrap();
        assert_eq!(rendered.width(), 600);

        // Images → PDF, then merge, split, rotate, first page.
        let pic = dir.path().join("pic.png");
        image::RgbaImage::from_pixel(80, 40, image::Rgba([255, 120, 0, 255]))
            .save(&pic)
            .unwrap();
        let from_image = images_to_pdf(std::slice::from_ref(&pic), &opts).unwrap();
        assert_eq!(page_count(&from_image).unwrap(), 1);
        let merged = merge(&[hello.clone(), from_image.clone()], &opts).unwrap();
        assert_eq!(page_count(&merged).unwrap(), 2);
        let parts = tool(&merged, "split", &opts).unwrap();
        assert_eq!(parts.len(), 2);
        assert!(text(&parts[0]).unwrap().contains("Bonjour"));
        let first = tool(&merged, "first-page", &opts).unwrap();
        assert_eq!(page_count(&first[0]).unwrap(), 1);
        let rotated = tool(&hello, "rotate-cw", &opts).unwrap();
        let doc = open(&rotated[0]).unwrap();
        assert_eq!(
            doc.pages().get(0).unwrap().rotation().unwrap(),
            PdfPageRenderRotation::Degrees90
        );
    }
}
