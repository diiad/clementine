//! Clémentine core: a local, offline file-conversion engine.
//!
//! The engine is organised around [`Operation`]s. Given a selection of files,
//! [`operations`] lists everything that can be done with it, grouped by
//! [`Category`] (convert, tools, actions), and [`run`] executes one of them.
//! Heavy lifting is delegated to pure-Rust crates where possible, to macOS
//! system frameworks (PDFKit, CoreGraphics, `textutil`, `sips`, `bsdtar`) and
//! to `ffmpeg` for audio and video.

pub mod engines;
pub mod kind;
mod output;
mod recipe;
mod registry;

pub use kind::{detect, Kind};
pub use output::{output_path, unique_path};
pub use recipe::{run_recipe, Recipe};
pub use registry::{catalog, operations, run, Category, Operation};

use std::path::PathBuf;

/// User-tunable settings that affect how jobs are executed.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Options {
    /// Where results are written. `None` writes next to the source file.
    pub out_dir: Option<PathBuf>,
    /// Lossy encoder quality, 1..=100.
    pub quality: u8,
    /// Resolution used when rasterising PDF and SVG files.
    pub dpi: u32,
    /// How videos are encoded when converted.
    pub video_preset: VideoPreset,
    /// Size `tool:fit-size` aims under, in megabytes (10⁶ bytes).
    pub target_mb: f64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            out_dir: None,
            quality: 85,
            dpi: 300,
            video_preset: VideoPreset::Standard,
            target_mb: 10.0,
        }
    }
}

/// Video encoding presets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoPreset {
    /// Follows the quality setting.
    #[default]
    Standard,
    /// At most 1080p, lighter, always re-encoded: for sharing on the web.
    Web,
    /// Near-lossless, slower to encode.
    Max,
}

/// Progress callback, receives a fraction in `0.0..=1.0`.
pub type Progress<'a> = &'a (dyn Fn(f32) + Sync);

pub fn no_progress(_: f32) {}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unknown operation `{0}`")]
    UnknownOperation(String),
    #[error("`{op}` is not available for {file}")]
    Unsupported { op: String, file: String },
    #[error("{0} is required for this conversion but was not found. {1}")]
    MissingTool(&'static str, &'static str),
    #[error("{tool} failed: {message}")]
    ToolFailed { tool: String, message: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
