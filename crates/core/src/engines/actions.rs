//! File actions that don't need a UI. AirDrop, Move… and Float need windows
//! or the main thread and are implemented by the app.

use super::exec;
use crate::{Error, Options, Result};
use std::path::PathBuf;

pub fn run(name: &str, inputs: &[PathBuf], _opts: &Options) -> Result<Vec<PathBuf>> {
    match name {
        "copy-path" => {
            let text = inputs
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n");
            copy_text(&text)?;
        }
        "copy" => copy_files(inputs)?,
        "terminal" => {
            let first = inputs
                .first()
                .ok_or_else(|| Error::UnknownOperation("action:terminal".into()))?;
            let dir = if first.is_dir() {
                first.clone()
            } else {
                first.parent().map(Into::into).unwrap_or_default()
            };
            open_terminal(&dir)?;
        }
        "reveal" => reveal(inputs)?,
        other => {
            return Err(Error::Unsupported {
                op: format!("action:{other}"),
                file: "the command line (open the Clémentine app)".into(),
            })
        }
    }
    Ok(Vec::new())
}

#[cfg(target_os = "macos")]
fn open_terminal(dir: &std::path::Path) -> Result<()> {
    exec(
        "/usr/bin/open",
        ["-a".as_ref(), "Terminal".as_ref(), dir.as_os_str()],
    )?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn reveal(paths: &[PathBuf]) -> Result<()> {
    let mut args = vec![std::ffi::OsString::from("-R")];
    args.extend(paths.iter().map(|p| p.clone().into_os_string()));
    exec("/usr/bin/open", args)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn copy_text(text: &str) -> Result<()> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::NSString;
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    if unsafe { pb.setString_forType(&NSString::from_str(text), NSPasteboardTypeString) } {
        Ok(())
    } else {
        Err(anyhow::anyhow!("could not write to the clipboard").into())
    }
}

/// Put the files themselves on the clipboard, so ⌘V in Finder pastes copies.
#[cfg(target_os = "macos")]
fn copy_files(paths: &[PathBuf]) -> Result<()> {
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{NSPasteboard, NSPasteboardWriting};
    use objc2_foundation::{NSArray, NSString, NSURL};
    let urls: Vec<Retained<NSURL>> = paths
        .iter()
        .map(|p| NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy())))
        .collect();
    let writers: Vec<&ProtocolObject<dyn NSPasteboardWriting>> = urls
        .iter()
        .map(|u| ProtocolObject::from_ref(&**u))
        .collect();
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    if pb.writeObjects(&NSArray::from_slice(&writers)) {
        Ok(())
    } else {
        Err(anyhow::anyhow!("could not write to the clipboard").into())
    }
}

// --- Windows ------------------------------------------------------------------

/// Run a PowerShell snippet. `-EncodedCommand` (UTF-16LE, base64) sidesteps
/// every quoting problem with paths.
#[cfg(windows)]
pub fn powershell(script: &str) -> Result<String> {
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64(&utf16);
    let out = exec(
        "powershell.exe",
        [
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            encoded.as_str(),
        ],
    )?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(windows)]
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

/// A PowerShell single-quoted string literal.
#[cfg(windows)]
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[cfg(windows)]
fn copy_text(text: &str) -> Result<()> {
    powershell(&format!("Set-Clipboard -Value {}", ps_quote(text)))?;
    Ok(())
}

/// Put the files on the clipboard, so Ctrl+V in Explorer pastes copies.
#[cfg(windows)]
fn copy_files(paths: &[PathBuf]) -> Result<()> {
    let list: Vec<String> = paths
        .iter()
        .map(|p| ps_quote(&p.to_string_lossy()))
        .collect();
    powershell(&format!("Set-Clipboard -LiteralPath {}", list.join(",")))?;
    Ok(())
}

#[cfg(windows)]
fn open_terminal(dir: &std::path::Path) -> Result<()> {
    use std::process::Command;
    // Windows Terminal when installed, otherwise a classic console.
    let wt = Command::new("wt.exe").arg("-d").arg(dir).spawn();
    if wt.is_err() {
        Command::new("cmd.exe")
            .args(["/C", "start", "", "/D"])
            .arg(dir)
            .arg("cmd.exe")
            .spawn()?;
    }
    Ok(())
}

#[cfg(windows)]
fn reveal(paths: &[PathBuf]) -> Result<()> {
    // explorer.exe exits with 1 even on success: don't check its status.
    if let Some(first) = paths.first() {
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(first);
        std::process::Command::new("explorer.exe")
            .arg(arg)
            .spawn()?;
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", windows)))]
fn unsupported() -> Result<()> {
    Err(Error::MissingTool(
        "a desktop",
        "Only macOS and Windows are supported.",
    ))
}
#[cfg(not(any(target_os = "macos", windows)))]
fn copy_text(_: &str) -> Result<()> {
    unsupported()
}
#[cfg(not(any(target_os = "macos", windows)))]
fn copy_files(_: &[PathBuf]) -> Result<()> {
    unsupported()
}
#[cfg(not(any(target_os = "macos", windows)))]
fn open_terminal(_: &std::path::Path) -> Result<()> {
    unsupported()
}
#[cfg(not(any(target_os = "macos", windows)))]
fn reveal(_: &[PathBuf]) -> Result<()> {
    unsupported()
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn base64_matches_reference() {
        assert_eq!(super::base64(b"Man"), "TWFu");
        assert_eq!(super::base64(b"Ma"), "TWE=");
        assert_eq!(super::base64(b"M"), "TQ==");
    }
}
