//! Still images: decoding with the `image` crate (HEIC/AVIF through macOS
//! `sips`), SVG through `resvg`, lossy WebP through libwebp.

use super::{exec, scratch};
use crate::kind::ext;
use crate::{output_path, Error, Options, Result};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Rgba, RgbaImage};
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

/// Formats the `image` crate decodes on its own.
const NATIVE: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "tiff", "tif", "bmp", "gif", "ico", "tga",
];

/// Whether [`decode`] can read this extension here. HEIC and AVIF go through
/// macOS's `sips`, or Windows' imaging codecs.
pub fn can_decode(e: &str) -> bool {
    e == "svg"
        || NATIVE.contains(&e)
        || cfg!(target_os = "macos")
        || cfg!(windows) && matches!(e, "heic" | "heif" | "avif")
}

pub fn decode(path: &Path, dpi: u32) -> Result<DynamicImage> {
    let e = ext(path);
    if e == "svg" {
        return decode_svg(path, dpi);
    }
    if NATIVE.contains(&e.as_str()) {
        return decode_native(path);
    }
    if cfg!(windows) {
        return decode_system(path);
    }
    // HEIC, AVIF and anything else ImageIO understands.
    let tmp = scratch()?;
    let png = tmp.path().join("decoded.png");
    exec(
        "/usr/bin/sips",
        [
            "-s".as_ref(),
            "format".as_ref(),
            "png".as_ref(),
            path.as_os_str(),
            "--out".as_ref(),
            png.as_os_str(),
        ],
    )?;
    decode_native(&png)
}

/// HEIC, AVIF… through Windows' imaging codecs.
#[cfg(windows)]
fn decode_system(path: &Path) -> Result<DynamicImage> {
    super::winrt::decode(path)
}

#[cfg(not(windows))]
fn decode_system(_: &Path) -> Result<DynamicImage> {
    unreachable!("macOS decodes through sips")
}

fn decode_native(path: &Path) -> Result<DynamicImage> {
    let mut decoder = ImageReader::open(path)?
        .with_guessed_format()?
        .into_decoder()
        .map_err(anyhow::Error::from)?;
    let orientation = decoder.orientation().map_err(anyhow::Error::from)?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(anyhow::Error::from)?;
    img.apply_orientation(orientation);
    Ok(img)
}

fn decode_svg(path: &Path, dpi: u32) -> Result<DynamicImage> {
    use resvg::{tiny_skia, usvg};
    let data = std::fs::read(path)?;
    let mut opt = usvg::Options {
        resources_dir: path.parent().map(Path::to_path_buf),
        ..Default::default()
    };
    opt.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_data(&data, &opt).map_err(anyhow::Error::from)?;
    // SVG user units are CSS pixels (96 dpi).
    let scale = (dpi as f32 / 96.0).max(1.0);
    let size = tree
        .size()
        .to_int_size()
        .scale_by(scale)
        .ok_or_else(|| anyhow::anyhow!("empty SVG"))?;
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height())
        .ok_or_else(|| anyhow::anyhow!("SVG too large"))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // tiny-skia stores premultiplied RGBA.
    let mut rgba = RgbaImage::new(size.width(), size.height());
    for (dst, src) in rgba.pixels_mut().zip(pixmap.pixels()) {
        let c = src.demultiply();
        *dst = Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Ok(DynamicImage::ImageRgba8(rgba))
}

/// Composite onto white, for formats without an alpha channel.
fn flatten(img: &DynamicImage) -> image::RgbImage {
    if !img.color().has_alpha() {
        return img.to_rgb8();
    }
    let rgba = img.to_rgba8();
    image::RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let p = rgba.get_pixel(x, y).0;
        let a = p[3] as u32;
        let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        image::Rgb([mix(p[0]), mix(p[1]), mix(p[2])])
    })
}

pub fn encode(img: &DynamicImage, target: &str, out: &Path, quality: u8) -> Result<()> {
    let q = quality.clamp(1, 100);
    let io = |e: image::ImageError| Error::Other(e.into());
    match target {
        "jpg" | "jpeg" => {
            let w = BufWriter::new(File::create(out)?);
            flatten(img)
                .write_with_encoder(JpegEncoder::new_with_quality(w, q))
                .map_err(io)?;
        }
        "png" => {
            let w = BufWriter::new(File::create(out)?);
            let enc = PngEncoder::new_with_quality(w, CompressionType::Best, PngFilter::Adaptive);
            img.write_with_encoder(enc).map_err(io)?;
        }
        "webp" => {
            let rgba = DynamicImage::ImageRgba8(img.to_rgba8());
            let enc = webp::Encoder::from_image(&rgba).map_err(|e| anyhow::anyhow!("{e}"))?;
            std::fs::write(out, &*enc.encode(q as f32))?;
        }
        "avif" => {
            let w = BufWriter::new(File::create(out)?);
            let enc = image::codecs::avif::AvifEncoder::new_with_speed_quality(w, 6, q);
            DynamicImage::ImageRgba8(img.to_rgba8())
                .write_with_encoder(enc)
                .map_err(io)?;
        }
        "heic" | "heif" => {
            let tmp = scratch()?;
            let png = tmp.path().join("in.png");
            img.save_with_format(&png, ImageFormat::Png).map_err(io)?;
            let fmt_q = format!("{}", q);
            exec(
                "/usr/bin/sips",
                [
                    "-s".as_ref(),
                    "format".as_ref(),
                    "heic".as_ref(),
                    "-s".as_ref(),
                    "formatOptions".as_ref(),
                    fmt_q.as_ref(),
                    png.as_os_str(),
                    "--out".as_ref(),
                    out.as_os_str(),
                ],
            )?;
        }
        "ico" => {
            let small = if img.width() > 256 || img.height() > 256 {
                img.resize(256, 256, FilterType::Lanczos3)
            } else {
                img.clone()
            };
            DynamicImage::ImageRgba8(small.to_rgba8())
                .save_with_format(out, ImageFormat::Ico)
                .map_err(io)?;
        }
        "icns" => {
            // Icons are square, power-of-two sized: centre on a transparent canvas.
            let side = img.width().max(img.height());
            let size = [1024u32, 512, 256, 128, 64, 32, 16]
                .into_iter()
                .find(|s| *s <= side)
                .unwrap_or(16);
            let fitted = img.resize(size, size, FilterType::Lanczos3).to_rgba8();
            let mut canvas = RgbaImage::new(size, size);
            image::imageops::overlay(
                &mut canvas,
                &fitted,
                ((size - fitted.width()) / 2).into(),
                ((size - fitted.height()) / 2).into(),
            );
            let tmp = scratch()?;
            let png = tmp.path().join("icon.png");
            canvas
                .save_with_format(&png, ImageFormat::Png)
                .map_err(io)?;
            exec(
                "/usr/bin/sips",
                [
                    "-s".as_ref(),
                    "format".as_ref(),
                    "icns".as_ref(),
                    png.as_os_str(),
                    "--out".as_ref(),
                    out.as_os_str(),
                ],
            )?;
        }
        "gif" => {
            DynamicImage::ImageRgba8(img.to_rgba8())
                .save_with_format(out, ImageFormat::Gif)
                .map_err(io)?;
        }
        "bmp" => DynamicImage::ImageRgb8(flatten(img))
            .save_with_format(out, ImageFormat::Bmp)
            .map_err(io)?,
        "tiff" | "tif" => img.save_with_format(out, ImageFormat::Tiff).map_err(io)?,
        "tga" => img.save_with_format(out, ImageFormat::Tga).map_err(io)?,
        other => return Err(anyhow::anyhow!("cannot encode {other}").into()),
    }
    Ok(())
}

pub fn convert(input: &Path, target: &str, opts: &Options) -> Result<PathBuf> {
    let img = decode(input, opts.dpi)?;
    let out = output_path(input, "", target, opts);
    encode(&img, target, &out, opts.quality)?;
    Ok(out)
}

/// Edit tools. The result keeps the source format.
pub fn tool(input: &Path, name: &str, opts: &Options) -> Result<PathBuf> {
    let img = decode(input, opts.dpi)?;
    let mut quality = opts.quality;
    let (img, suffix) = match name {
        "compress" => {
            quality = quality.min(70);
            (img, "-compressed")
        }
        "resize-50" => {
            let (w, h) = (img.width().div_ceil(2), img.height().div_ceil(2));
            (img.resize_exact(w, h, FilterType::Lanczos3), "-half")
        }
        "resize-1080" => {
            if img.width().max(img.height()) > 1080 {
                (img.resize(1080, 1080, FilterType::Lanczos3), "-1080")
            } else {
                (img, "-1080")
            }
        }
        "rotate-cw" => (img.rotate90(), "-rotated"),
        "rotate-ccw" => (img.rotate270(), "-rotated"),
        "flip-h" => (img.fliph(), "-flipped"),
        "grayscale" => (img.grayscale(), "-gray"),
        "strip-metadata" => (img, "-clean"),
        _ => {
            return Err(Error::Unsupported {
                op: format!("tool:{name}"),
                file: input.display().to_string(),
            })
        }
    };
    let mut target = ext(input);
    if target == "svg" {
        target = "png".into();
    }
    let out = output_path(input, suffix, &target, opts);
    encode(&img, &target, &out, quality)?;
    Ok(out)
}

/// Keep the `w`×`h` rectangle at `x`,`y` (pixels of the upright image,
/// clamped to it). The result keeps the source format.
pub fn crop(input: &Path, x: u32, y: u32, w: u32, h: u32, opts: &Options) -> Result<PathBuf> {
    let img = decode(input, opts.dpi)?;
    let x = x.min(img.width().saturating_sub(1));
    let y = y.min(img.height().saturating_sub(1));
    let w = w.clamp(1, img.width() - x);
    let h = h.clamp(1, img.height() - y);
    let cropped = img.crop_imm(x, y, w, h);
    let mut target = ext(input);
    if target == "svg" {
        target = "png".into();
    }
    let out = output_path(input, "-cropped", &target, opts);
    encode(&cropped, &target, &out, opts.quality)?;
    Ok(out)
}

/// Re-encode so the file fits in `opts.target_mb`: the highest JPEG quality
/// that fits (WebP for images with transparency), shrinking the image when
/// even a low quality is too big.
pub fn fit_size(input: &Path, opts: &Options) -> Result<PathBuf> {
    let (limit, suffix) = super::media::target_size(opts);
    let mut img = decode(input, opts.dpi)?;
    let alpha = img.color().has_alpha() && img.to_rgba8().pixels().any(|p| p.0[3] < 255);
    let target = if alpha { "webp" } else { "jpg" };
    let out = output_path(input, &suffix, target, opts);
    // Already small enough: re-encoding could only lose quality.
    if std::fs::metadata(input)?.len() <= limit && crate::registry::same_format(target, &ext(input))
    {
        std::fs::copy(input, &out)?;
        return Ok(out);
    }
    let tmp = scratch()?;
    let probe = tmp.path().join(format!("probe.{target}"));
    let size = |img: &DynamicImage, q: u8| -> Result<u64> {
        encode(img, target, &probe, q)?;
        Ok(std::fs::metadata(&probe)?.len())
    };
    for _ in 0..8 {
        // Binary search for the best quality in 30..=95 that fits.
        let (mut lo, mut hi, mut best) = (30u8, 95u8, None);
        while lo <= hi {
            let q = lo + (hi - lo) / 2;
            if size(&img, q)? <= limit {
                best = Some(q);
                lo = q + 1;
            } else {
                hi = q - 1;
            }
        }
        if let Some(q) = best {
            encode(&img, target, &out, q)?;
            return Ok(out);
        }
        let ratio = (limit as f64 / size(&img, 30)? as f64).sqrt() * 0.9;
        let (w, h) = (img.width() as f64 * ratio, img.height() as f64 * ratio);
        if w < 16.0 || h < 16.0 {
            break;
        }
        img = img.resize(w as u32, h as u32, FilterType::Lanczos3);
    }
    Err(anyhow::anyhow!("could not fit {} in {} MB", input.display(), opts.target_mb).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(dir: &Path) -> PathBuf {
        let img = RgbaImage::from_fn(40, 20, |x, y| Rgba([x as u8 * 6, y as u8 * 12, 128, 200]));
        let p = dir.join("sample.png");
        img.save(&p).unwrap();
        p
    }

    #[test]
    fn png_round_trips_through_lossy_formats() {
        let dir = tempfile::tempdir().unwrap();
        let src = sample(dir.path());
        let opts = Options::default();
        for target in ["jpg", "webp", "bmp", "tiff", "gif", "ico", "avif"] {
            let out = convert(&src, target, &opts).unwrap();
            // AVIF is written everywhere but read through the system: sips
            // on macOS, an optional Store extension on Windows.
            if target == "avif" && !cfg!(target_os = "macos") {
                continue;
            }
            let back = decode_native(&out).or_else(|_| decode(&out, 72)).unwrap();
            assert_eq!((back.width(), back.height()), (40, 20), "{target}");
        }
    }

    #[test]
    fn tools_change_geometry() {
        let dir = tempfile::tempdir().unwrap();
        let src = sample(dir.path());
        let opts = Options::default();
        let rotated = decode_native(&tool(&src, "rotate-cw", &opts).unwrap()).unwrap();
        assert_eq!((rotated.width(), rotated.height()), (20, 40));
        let half = decode_native(&tool(&src, "resize-50", &opts).unwrap()).unwrap();
        assert_eq!((half.width(), half.height()), (20, 10));
        let cut = decode_native(&crop(&src, 30, 5, 100, 10, &opts).unwrap()).unwrap();
        assert_eq!((cut.width(), cut.height()), (10, 10));
    }

    #[test]
    fn fits_target_size() {
        let dir = tempfile::tempdir().unwrap();
        // Noise compresses badly: about 1 MB as a high-quality JPEG.
        let mut seed = 1u32;
        let img = image::RgbImage::from_fn(1200, 900, |_, _| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let b = seed.to_le_bytes();
            image::Rgb([b[1], b[2], b[3]])
        });
        let src = dir.path().join("noise.png");
        img.save(&src).unwrap();
        let opts = Options {
            target_mb: 0.2,
            ..Default::default()
        };
        let out = fit_size(&src, &opts).unwrap();
        assert_eq!(out.file_name().unwrap(), "noise-0.2MB.jpg");
        assert!(std::fs::metadata(&out).unwrap().len() <= 200_000);
    }

    #[test]
    fn svg_is_rasterised() {
        let dir = tempfile::tempdir().unwrap();
        let svg = dir.path().join("logo.svg");
        std::fs::write(&svg, r#"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="48"><rect width="96" height="48" fill="orange"/></svg>"#).unwrap();
        let png = convert(
            &svg,
            "png",
            &Options {
                dpi: 192,
                ..Default::default()
            },
        )
        .unwrap();
        let img = decode_native(&png).unwrap();
        assert_eq!((img.width(), img.height()), (192, 96));
    }
}
