//! PDF handling through PDFKit and CoreGraphics (macOS only).

use crate::kind::{ext, kind_of_ext, Kind};
use crate::{output_path, Error, Options, Progress, Result};
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use objc2::rc::Retained;
    use objc2::AllocAnyThread;
    use objc2_app_kit::{NSAttributedStringDocumentFormats, NSImage};
    use objc2_core_foundation::{CFRetained, CGFloat, CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{CGColorSpace, CGContext};
    use objc2_foundation::{NSDictionary, NSMutableAttributedString, NSRange, NSString, NSURL};
    use objc2_pdf_kit::{PDFDisplayBox, PDFDocument, PDFPage};
    use std::ffi::c_void;
    use std::ptr::NonNull;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGBitmapContextCreate(
            data: *mut c_void,
            width: usize,
            height: usize,
            bits_per_component: usize,
            bytes_per_row: usize,
            space: *const CGColorSpace,
            bitmap_info: u32,
        ) -> *mut CGContext;
    }

    fn url(path: &Path) -> Retained<NSURL> {
        NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
    }

    pub fn open(path: &Path) -> Result<Retained<PDFDocument>> {
        unsafe { PDFDocument::initWithURL(PDFDocument::alloc(), &url(path)) }
            .ok_or_else(|| anyhow::anyhow!("could not open {} as a PDF", path.display()).into())
    }

    pub fn new_doc() -> Retained<PDFDocument> {
        unsafe { PDFDocument::new() }
    }

    pub fn page_count(doc: &PDFDocument) -> usize {
        unsafe { doc.pageCount() }
    }

    pub fn page(doc: &PDFDocument, i: usize) -> Result<Retained<PDFPage>> {
        unsafe { doc.pageAtIndex(i) }.ok_or_else(|| anyhow::anyhow!("missing page {i}").into())
    }

    /// Append a copy of `page` to `doc` (pages can only belong to one document).
    pub fn push_copy(doc: &PDFDocument, page: &PDFPage) {
        let copy: Retained<PDFPage> = objc2_foundation::NSCopying::copy(page);
        unsafe { doc.insertPage_atIndex(&copy, doc.pageCount()) };
    }

    pub fn rotate(page: &PDFPage, degrees: isize) {
        unsafe { page.setRotation((page.rotation() + degrees).rem_euclid(360)) };
    }

    pub fn write(doc: &PDFDocument, out: &Path) -> Result<()> {
        if unsafe { doc.writeToURL(&url(out)) } {
            Ok(())
        } else {
            Err(anyhow::anyhow!("could not write {}", out.display()).into())
        }
    }

    /// Re-save with images downsampled and JPEG-encoded (macOS 13+).
    pub fn write_compressed(doc: &PDFDocument, out: &Path) -> Result<()> {
        use objc2::runtime::AnyObject;
        use objc2_foundation::NSNumber;
        use objc2_pdf_kit::{
            PDFDocumentOptimizeImagesForScreenOption, PDFDocumentSaveImagesAsJPEGOption,
        };
        let yes = NSNumber::new_bool(true);
        let opts: Retained<NSDictionary<NSString, AnyObject>> = unsafe {
            NSDictionary::from_slices(
                &[
                    PDFDocumentSaveImagesAsJPEGOption,
                    PDFDocumentOptimizeImagesForScreenOption,
                ],
                &[yes.as_ref() as &AnyObject, yes.as_ref() as &AnyObject],
            )
        };
        if unsafe { doc.writeToURL_withOptions(&url(out), Some(&opts)) } {
            Ok(())
        } else {
            Err(anyhow::anyhow!("could not write {}", out.display()).into())
        }
    }

    pub fn page_from_image(path: &Path) -> Result<Retained<PDFPage>> {
        let image = NSImage::initWithContentsOfURL(NSImage::alloc(), &url(path))
            .ok_or_else(|| anyhow::anyhow!("could not read image {}", path.display()))?;
        unsafe { PDFPage::initWithImage(PDFPage::alloc(), &image) }.ok_or_else(|| {
            anyhow::anyhow!("could not make a PDF page from {}", path.display()).into()
        })
    }

    /// Rasterise one page at `dpi` onto a white background.
    pub fn render(page: &PDFPage, dpi: u32) -> Result<image::RgbaImage> {
        let bounds = unsafe { page.boundsForBox(PDFDisplayBox::CropBox) };
        let rotated = unsafe { page.rotation() }.rem_euclid(180) == 90;
        let (pw, ph) = if rotated {
            (bounds.size.height, bounds.size.width)
        } else {
            (bounds.size.width, bounds.size.height)
        };
        let scale = dpi as CGFloat / 72.0;
        let (w, h) = ((pw * scale).round() as usize, (ph * scale).round() as usize);
        if w == 0 || h == 0 || w * h > 30_000 * 30_000 {
            return Err(anyhow::anyhow!("page size out of range").into());
        }
        let mut buf = vec![0u8; w * h * 4];
        let space =
            CGColorSpace::new_device_rgb().ok_or_else(|| anyhow::anyhow!("no RGB colour space"))?;
        // kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big → RGBA in memory.
        let info = 1 | (4 << 12);
        let raw = unsafe {
            CGBitmapContextCreate(buf.as_mut_ptr().cast(), w, h, 8, w * 4, &*space, info)
        };
        let ctx: CFRetained<CGContext> = unsafe {
            CFRetained::from_raw(
                NonNull::new(raw).ok_or_else(|| anyhow::anyhow!("could not allocate bitmap"))?,
            )
        };
        CGContext::set_rgb_fill_color(Some(&ctx), 1.0, 1.0, 1.0, 1.0);
        CGContext::fill_rect(
            Some(&ctx),
            CGRect::new(
                CGPoint::new(0.0, 0.0),
                CGSize::new(w as CGFloat, h as CGFloat),
            ),
        );
        CGContext::scale_ctm(Some(&ctx), scale, scale);
        unsafe {
            page.transformContext_forBox(&ctx, PDFDisplayBox::CropBox);
            page.drawWithBox_toContext(PDFDisplayBox::CropBox, &ctx);
        }
        drop(ctx);
        image::RgbaImage::from_raw(w as u32, h as u32, buf)
            .ok_or_else(|| anyhow::anyhow!("bitmap size mismatch").into())
    }

    #[link(name = "CoreText", kind = "framework")]
    extern "C" {
        fn CTFramesetterCreateWithAttributedString(s: *const c_void) -> *mut c_void;
        fn CTFramesetterCreateFrame(
            fs: *mut c_void,
            range: CFRange,
            path: *const c_void,
            attrs: *const c_void,
        ) -> *mut c_void;
        fn CTFrameGetVisibleStringRange(frame: *mut c_void) -> CFRange;
        fn CTFrameDraw(frame: *mut c_void, ctx: *mut c_void);
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPDFContextCreateWithURL(
            url: *const c_void,
            media_box: *const CGRect,
            info: *const c_void,
        ) -> *mut c_void;
        fn CGPDFContextBeginPage(ctx: *mut c_void, info: *const c_void);
        fn CGPDFContextEndPage(ctx: *mut c_void);
        fn CGPDFContextClose(ctx: *mut c_void);
        fn CGPathCreateWithRect(rect: CGRect, transform: *const c_void) -> *mut c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(obj: *mut c_void);
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CFRange {
        location: isize,
        length: isize,
    }

    /// Typeset RTF onto A4 pages with CoreText. Thread-safe, unlike printing
    /// through an NSTextView.
    pub fn typeset_rtf(rtf: &[u8], out: &Path) -> Result<()> {
        use objc2_app_kit::NSAttributedStringAppKitDocumentFormats;
        use objc2_foundation::{NSAttributedString, NSData};
        let data = NSData::with_bytes(rtf);
        let text = unsafe {
            NSAttributedString::initWithRTF_documentAttributes(
                NSAttributedString::alloc(),
                &data,
                None,
            )
        }
        .ok_or_else(|| anyhow::anyhow!("could not read the document text"))?;
        let total = text.length() as isize;
        let page = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(595.0, 842.0));
        let margin = 64.0;
        let body = CGRect::new(
            CGPoint::new(margin, margin),
            CGSize::new(
                page.size.width - 2.0 * margin,
                page.size.height - 2.0 * margin,
            ),
        );
        let out_url = url(out);
        unsafe {
            let fs = CTFramesetterCreateWithAttributedString(Retained::as_ptr(&text).cast());
            let ctx = CGPDFContextCreateWithURL(
                Retained::as_ptr(&out_url).cast(),
                &page,
                std::ptr::null(),
            );
            if fs.is_null() || ctx.is_null() {
                return Err(anyhow::anyhow!("could not create the PDF").into());
            }
            let path = CGPathCreateWithRect(body, std::ptr::null());
            let mut start = 0isize;
            loop {
                CGPDFContextBeginPage(ctx, std::ptr::null());
                let frame = CTFramesetterCreateFrame(
                    fs,
                    CFRange {
                        location: start,
                        length: 0,
                    },
                    path,
                    std::ptr::null(),
                );
                CTFrameDraw(frame, ctx);
                let visible = CTFrameGetVisibleStringRange(frame);
                CFRelease(frame);
                CGPDFContextEndPage(ctx);
                start += visible.length;
                if visible.length == 0 || start >= total {
                    break;
                }
            }
            CGPDFContextClose(ctx);
            CFRelease(path);
            CFRelease(ctx);
            CFRelease(fs);
        }
        Ok(())
    }

    pub fn text(doc: &PDFDocument) -> String {
        unsafe { doc.string() }
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// The text of every page, with fonts and styles, as RTF.
    pub fn rtf(doc: &PDFDocument) -> Result<Vec<u8>> {
        let all = NSMutableAttributedString::new();
        for i in 0..page_count(doc) {
            if let Some(s) = unsafe { page(doc, i)?.attributedString() } {
                all.appendAttributedString(&s);
                let br = objc2_foundation::NSAttributedString::from_nsstring(&NSString::from_str(
                    "\n\n",
                ));
                all.appendAttributedString(&br);
            }
        }
        let range = NSRange::new(0, all.length());
        let doc_type = unsafe { objc2_app_kit::NSRTFTextDocumentType };
        let key = unsafe { objc2_app_kit::NSDocumentTypeDocumentAttribute };
        let attrs = NSDictionary::from_slices(&[key], &[doc_type as &objc2::runtime::AnyObject]);
        let data = unsafe { all.dataFromRange_documentAttributes_error(range, &attrs) }
            .map_err(|e| anyhow::anyhow!("{}", e.localizedDescription()))?;
        Ok(data.to_vec())
    }
}

#[cfg(target_os = "macos")]
pub use mac::render;

#[cfg(not(target_os = "macos"))]
mod mac {
    use super::*;
    pub fn unsupported<T>() -> Result<T> {
        Err(Error::MissingTool(
            "PDFKit",
            "PDF features currently require macOS.",
        ))
    }
}

macro_rules! macos_only {
    ($body:block) => {{
        #[cfg(target_os = "macos")]
        {
            $body
        }
        #[cfg(not(target_os = "macos"))]
        {
            mac::unsupported()
        }
    }};
}

pub fn page_count(input: &Path) -> Result<usize> {
    macos_only!({ Ok(mac::page_count(&*mac::open(input)?)) })
}

/// One image per page: `doc.png` for single pages, `doc-1.png`, `doc-2.png`… otherwise.
pub fn to_images(
    input: &Path,
    target: &str,
    opts: &Options,
    progress: Progress,
) -> Result<Vec<PathBuf>> {
    macos_only!({
        let doc = mac::open(input)?;
        let n = mac::page_count(&doc);
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let img =
                image::DynamicImage::ImageRgba8(mac::render(&*mac::page(&doc, i)?, opts.dpi)?);
            let suffix = if n == 1 {
                String::new()
            } else {
                format!("-{}", i + 1)
            };
            let path = output_path(input, &suffix, target, opts);
            super::image::encode(&img, target, &path, opts.quality)?;
            out.push(path);
            progress((i + 1) as f32 / n as f32);
        }
        Ok(out)
    })
}

/// Scanned PDF → `.txt`: render each page and run Vision's OCR on it.
pub fn ocr(input: &Path, opts: &Options, progress: Progress) -> Result<PathBuf> {
    macos_only!({
        let doc = mac::open(input)?;
        let n = mac::page_count(&doc);
        let tmp = super::scratch()?;
        let png = tmp.path().join("page.png");
        let mut pages = Vec::with_capacity(n);
        for i in 0..n {
            // 200 dpi is plenty for OCR and keeps big scans fast.
            let page = mac::render(&*mac::page(&doc, i)?, opts.dpi.min(200))?;
            super::image::encode(&image::DynamicImage::ImageRgba8(page), "png", &png, 100)?;
            pages.push(super::vision::recognize_text(&png)?);
            progress((i + 1) as f32 / n as f32);
        }
        if pages.iter().all(|p| p.trim().is_empty()) {
            return Err(anyhow::anyhow!("no text found").into());
        }
        let out = output_path(input, "", "txt", opts);
        std::fs::write(&out, pages.join("\n\n") + "\n")?;
        Ok(out)
    })
}

pub fn text(input: &Path) -> Result<String> {
    macos_only!({ Ok(mac::text(&*mac::open(input)?)) })
}

pub fn rtf(input: &Path) -> Result<Vec<u8>> {
    macos_only!({ mac::rtf(&*mac::open(input)?) })
}

/// Lay out an RTF document onto A4 pages.
pub fn from_rtf(rtf: &[u8], out: &Path) -> Result<()> {
    macos_only!({ mac::typeset_rtf(rtf, out) })
}

pub fn merge(inputs: &[PathBuf], opts: &Options) -> Result<PathBuf> {
    macos_only!({
        let merged = mac::new_doc();
        for input in inputs {
            let doc = mac::open(input)?;
            for i in 0..mac::page_count(&doc) {
                mac::push_copy(&merged, &*mac::page(&doc, i)?);
            }
        }
        let out = output_path(&inputs[0], "-merged", "pdf", opts);
        mac::write(&merged, &out)?;
        Ok(out)
    })
}

/// Combine images (any format ImageIO reads, plus SVG) into one PDF, one page each.
pub fn images_to_pdf(inputs: &[PathBuf], opts: &Options) -> Result<PathBuf> {
    macos_only!({
        let tmp = super::scratch()?;
        let doc = mac::new_doc();
        for (i, input) in inputs.iter().enumerate() {
            let source = if kind_of_ext(&ext(input)) == Kind::Svg {
                let png = tmp.path().join(format!("{i}.png"));
                let img = super::image::decode(input, opts.dpi.min(192))?;
                super::image::encode(&img, "png", &png, 100)?;
                png
            } else {
                input.clone()
            };
            let page = mac::page_from_image(&source)?;
            mac::push_copy(&doc, &page);
        }
        let suffix = if inputs.len() > 1 { "-combined" } else { "" };
        let out = output_path(&inputs[0], suffix, "pdf", opts);
        mac::write(&doc, &out)?;
        Ok(out)
    })
}

pub fn tool(input: &Path, name: &str, opts: &Options) -> Result<Vec<PathBuf>> {
    macos_only!({
        let doc = mac::open(input)?;
        let n = mac::page_count(&doc);
        match name {
            "compress" => {
                let out = output_path(input, "-compressed", "pdf", opts);
                mac::write_compressed(&doc, &out)?;
                Ok(vec![out])
            }
            "rotate-cw" => {
                for i in 0..n {
                    mac::rotate(&*mac::page(&doc, i)?, 90);
                }
                let out = output_path(input, "-rotated", "pdf", opts);
                mac::write(&doc, &out)?;
                Ok(vec![out])
            }
            "split" => {
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let single = mac::new_doc();
                    mac::push_copy(&single, &*mac::page(&doc, i)?);
                    let path = output_path(input, &format!("-p{}", i + 1), "pdf", opts);
                    mac::write(&single, &path)?;
                    out.push(path);
                }
                Ok(out)
            }
            "first-page" => {
                let single = mac::new_doc();
                mac::push_copy(&single, &*mac::page(&doc, 0)?);
                let path = output_path(input, "-p1", "pdf", opts);
                mac::write(&single, &path)?;
                Ok(vec![path])
            }
            _ => Err(Error::Unsupported {
                op: format!("tool:{name}"),
                file: input.display().to_string(),
            }),
        }
    })
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    /// Make a two-page PDF from two generated PNGs.
    fn sample_pdf(dir: &Path) -> PathBuf {
        let mut pngs = Vec::new();
        for (i, c) in [[255u8, 0, 0, 255], [0, 0, 255, 255]]
            .into_iter()
            .enumerate()
        {
            let p = dir.join(format!("page{i}.png"));
            image::RgbaImage::from_pixel(144, 72, image::Rgba(c))
                .save(&p)
                .unwrap();
            pngs.push(p);
        }
        images_to_pdf(&pngs, &Options::default()).unwrap()
    }

    #[test]
    fn builds_renders_and_edits_pdfs() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = sample_pdf(dir.path());
        assert_eq!(page_count(&pdf).unwrap(), 2);

        let opts = Options {
            dpi: 144,
            ..Default::default()
        };
        let pngs = to_images(&pdf, "png", &opts, &|_| {}).unwrap();
        assert_eq!(pngs.len(), 2);
        let first = image::open(&pngs[0]).unwrap().to_rgba8();
        let centre = first.get_pixel(first.width() / 2, first.height() / 2).0;
        assert!(
            centre[0] > 200 && centre[2] < 60,
            "page 1 should be red, got {centre:?}"
        );

        let merged = merge(&[pdf.clone(), pdf.clone()], &opts).unwrap();
        assert_eq!(page_count(&merged).unwrap(), 4);
        assert_eq!(tool(&pdf, "split", &opts).unwrap().len(), 2);
        let rotated = tool(&pdf, "rotate-cw", &opts).unwrap().remove(0);
        let img = to_images(
            &rotated,
            "png",
            &Options {
                dpi: 72,
                ..Default::default()
            },
            &|_| {},
        )
        .unwrap();
        let r = image::open(&img[0]).unwrap();
        assert!(r.height() > r.width(), "rotated page should be portrait");
        assert!(tool(&pdf, "compress", &opts).unwrap()[0].exists());
    }
}
