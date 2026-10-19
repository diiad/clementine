//! Conversion back-ends, one module per family of formats.

pub mod actions;
pub mod archive;
pub mod document;
pub mod image;
pub mod media;
pub mod pdf;
pub mod sheet;
pub mod subtitle;
pub mod vision;

use crate::{Error, Result};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Run an external program and turn a non-zero exit into a readable error.
pub(crate) fn exec<I, S>(program: impl AsRef<OsStr>, args: I) -> Result<std::process::Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let program = program.as_ref();
    let name = Path::new(program)
        .file_name()
        .unwrap_or(program)
        .to_string_lossy()
        .into_owned();
    let out = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| Error::ToolFailed {
            tool: name.clone(),
            message: e.to_string(),
        })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let message = stderr
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("exited with an error")
            .trim()
            .to_string();
        return Err(Error::ToolFailed {
            tool: name,
            message,
        });
    }
    Ok(out)
}

/// Folders where tools usually live but that a GUI app's `$PATH` may lack:
/// Homebrew on macOS, the usual install folders on Windows.
fn tool_dirs() -> Vec<PathBuf> {
    if cfg!(windows) {
        let mut dirs = Vec::new();
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(pf) = std::env::var_os(var).map(PathBuf::from) {
                dirs.push(pf.join("LibreOffice").join("program"));
                dirs.push(pf.join("ffmpeg").join("bin"));
            }
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            dirs.push(local.join("Microsoft").join("WinGet").join("Links"));
        }
        dirs.push(PathBuf::from(r"C:\ffmpeg\bin"));
        dirs
    } else {
        ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"]
            .into_iter()
            .map(PathBuf::from)
            .collect()
    }
}

/// Locate an executable: `$CLEMENTINE_<NAME>`, next to our own binary (bundled
/// sidecar), well-known install folders, then `$PATH`.
pub fn find_tool(name: &str) -> Option<PathBuf> {
    let env_key = format!("CLEMENTINE_{}", name.to_uppercase().replace('-', "_"));
    if let Some(p) = std::env::var_os(env_key).map(PathBuf::from) {
        if p.is_file() {
            return Some(p);
        }
    }
    let file = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(&file));
            candidates.push(dir.join("../Resources").join(&file));
        }
    }
    for dir in tool_dirs() {
        candidates.push(dir.join(&file));
    }
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .or_else(|| which::which(name).ok())
}

/// Scratch directory that is removed when dropped.
pub(crate) fn scratch() -> Result<tempfile::TempDir> {
    Ok(tempfile::Builder::new().prefix("clementine-").tempdir()?)
}
