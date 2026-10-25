//! Windows' own imaging and OCR (WinRT), the counterparts of `sips` and
//! Vision on macOS. `BitmapDecoder` reads whatever WIC codecs are installed:
//! HEIC with Microsoft's *HEIF Image Extensions*, AVIF with *AV1 Video
//! Extension*. `OcrEngine` uses the languages of the user's profile.

use crate::{Error, Result};
use image::{DynamicImage, RgbaImage};
use std::path::Path;
use windows::core::HSTRING;
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapDecoder, BitmapInterpolationMode, BitmapPixelFormat, BitmapTransform,
    ColorManagementMode, ExifOrientationMode,
};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::{FileAccessMode, StorageFile};

fn err(e: windows::core::Error) -> Error {
    anyhow::anyhow!("Windows: {}", e.message()).into()
}

fn decoder(path: &Path) -> Result<BitmapDecoder> {
    // WinRT wants an absolute path.
    let path = std::path::absolute(path)?;
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))
        .and_then(|op| op.join())
        .map_err(err)?;
    let stream = file
        .OpenAsync(FileAccessMode::Read)
        .and_then(|op| op.join())
        .map_err(err)?;
    BitmapDecoder::CreateAsync(&stream)
        .and_then(|op| op.join())
        .map_err(err)
}

/// Decode an image with the installed WIC codecs, upright and in sRGB.
pub fn decode(path: &Path) -> Result<DynamicImage> {
    let missing = |e: Error| -> Error {
        let ext = crate::kind::ext(path);
        let hint = match ext.as_str() {
            "heic" | "heif" => " Install “HEIF Image Extensions” from the Microsoft Store.",
            "avif" => " Install “AV1 Video Extension” from the Microsoft Store.",
            _ => "",
        };
        anyhow::anyhow!("cannot read this {} file: {e}.{hint}", ext.to_uppercase()).into()
    };
    let decoder = decoder(path).map_err(missing)?;
    let (w, h) = (
        decoder.OrientedPixelWidth().map_err(err)?,
        decoder.OrientedPixelHeight().map_err(err)?,
    );
    let pixels = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Rgba8,
            BitmapAlphaMode::Straight,
            &BitmapTransform::new().map_err(err)?,
            ExifOrientationMode::RespectExifOrientation,
            ColorManagementMode::ColorManageToSRgb,
        )
        .and_then(|op| op.join())
        .and_then(|data| data.DetachPixelData())
        .map_err(err)?;
    RgbaImage::from_raw(w, h, pixels.to_vec())
        .map(DynamicImage::ImageRgba8)
        .ok_or_else(|| anyhow::anyhow!("unexpected pixel data size").into())
}

/// Whether an OCR engine exists for the user's languages.
pub fn ocr_available() -> bool {
    OcrEngine::TryCreateFromUserProfileLanguages().is_ok()
}

/// Recognise the text of an image file, line by line.
pub fn recognize_text(path: &Path) -> Result<String> {
    let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| {
        Error::Other(anyhow::anyhow!(
            "no OCR language installed: add one in Settings → Time & language → Language"
        ))
    })?;
    let decoder = decoder(path)?;
    // The engine refuses images larger than this: scale them down.
    let max = OcrEngine::MaxImageDimension().map_err(err)?;
    let (w, h) = (
        decoder.OrientedPixelWidth().map_err(err)?,
        decoder.OrientedPixelHeight().map_err(err)?,
    );
    let transform = BitmapTransform::new().map_err(err)?;
    if w.max(h) > max {
        let k = max as f64 / w.max(h) as f64;
        transform
            .SetScaledWidth(((w as f64 * k) as u32).max(1))
            .and_then(|_| transform.SetScaledHeight(((h as f64 * k) as u32).max(1)))
            .and_then(|_| transform.SetInterpolationMode(BitmapInterpolationMode::Fant))
            .map_err(err)?;
    }
    let bitmap = decoder
        .GetSoftwareBitmapTransformedAsync(
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Premultiplied,
            &transform,
            ExifOrientationMode::RespectExifOrientation,
            ColorManagementMode::ColorManageToSRgb,
        )
        .and_then(|op| op.join())
        .map_err(err)?;
    let result = engine
        .RecognizeAsync(&bitmap)
        .and_then(|op| op.join())
        .map_err(err)?;
    let mut lines = Vec::new();
    for line in result.Lines().map_err(err)? {
        lines.push(line.Text().map_err(err)?.to_string());
    }
    Ok(lines.join("\n"))
}
