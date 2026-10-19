//! "Install" buttons for the optional engines: Homebrew on macOS, winget on
//! Windows, or the download page when neither is there.

use clementine_core::engines::{command, find_tool};
use std::path::PathBuf;

#[derive(Clone, Copy)]
pub enum Engine {
    Ffmpeg,
    LibreOffice,
}

impl Engine {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "ffmpeg" => Some(Self::Ffmpeg),
            "libreoffice" => Some(Self::LibreOffice),
            _ => None,
        }
    }

    fn download_page(self) -> &'static str {
        match self {
            Self::Ffmpeg => "https://ffmpeg.org/download.html",
            Self::LibreOffice => "https://www.libreoffice.org/download/download-libreoffice/",
        }
    }

    /// Package-manager arguments after the program name.
    fn package_args(self) -> Vec<&'static str> {
        if cfg!(windows) {
            let id = match self {
                Self::Ffmpeg => "Gyan.FFmpeg",
                Self::LibreOffice => "TheDocumentFoundation.LibreOffice",
            };
            vec![
                "install",
                "--id",
                id,
                "--exact",
                "--silent",
                "--accept-package-agreements",
                "--accept-source-agreements",
            ]
        } else {
            match self {
                Self::Ffmpeg => vec!["install", "ffmpeg"],
                Self::LibreOffice => vec!["install", "--cask", "libreoffice"],
            }
        }
    }
}

/// What the button did.
#[derive(serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Installed,
    /// No package manager: the download page opened in the browser.
    Opened,
}

/// Homebrew or winget. A GUI app's `$PATH` lacks Homebrew, so look in its
/// usual folders too.
fn package_manager() -> Option<PathBuf> {
    if cfg!(windows) {
        find_tool("winget")
    } else {
        ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]
            .into_iter()
            .map(PathBuf::from)
            .find(|p| p.is_file())
    }
}

fn open_url(url: &str) -> std::io::Result<()> {
    let mut cmd = if cfg!(windows) {
        let mut c = command("cmd.exe");
        c.args(["/C", "start", "", url]);
        c
    } else {
        let mut c = command("/usr/bin/open");
        c.arg(url);
        c
    };
    cmd.status().map(|_| ())
}

/// Blocking: an install can take minutes (LibreOffice is ~350 MB).
pub fn install(engine: Engine) -> Result<Outcome, String> {
    let Some(pm) = package_manager() else {
        open_url(engine.download_page()).map_err(|e| e.to_string())?;
        return Ok(Outcome::Opened);
    };
    let out = command(&pm)
        .args(engine.package_args())
        .env("HOMEBREW_NO_AUTO_UPDATE", "1")
        .env("HOMEBREW_NO_ENV_HINTS", "1")
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(Outcome::Installed);
    }
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Err(text
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .unwrap_or("installation failed")
        .to_string())
}
