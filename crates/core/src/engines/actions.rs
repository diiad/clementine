//! File actions that don't need a UI. AirDrop, Move… and Float need windows
//! or the main thread and are implemented by the app.

use super::exec;
use crate::{Error, Options, Result};
use std::path::PathBuf;

pub fn run(name: &str, inputs: &[PathBuf], _opts: &Options) -> Result<Vec<PathBuf>> {
    match name {
        "copy-path" => {
            let text = inputs.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n");
            copy_text(&text)?;
        }
        "copy" => copy_files(inputs)?,
        "terminal" => {
            let first = inputs.first().ok_or_else(|| Error::UnknownOperation("action:terminal".into()))?;
            let dir = if first.is_dir() { first.clone() } else { first.parent().map(Into::into).unwrap_or_default() };
            exec("/usr/bin/open", ["-a".as_ref(), "Terminal".as_ref(), dir.as_os_str()])?;
        }
        "reveal" => {
            let mut args = vec![std::ffi::OsString::from("-R")];
            args.extend(inputs.iter().map(|p| p.clone().into_os_string()));
            exec("/usr/bin/open", args)?;
        }
        other => {
            return Err(Error::Unsupported {
                op: format!("action:{other}"),
                file: "the command line (open the Satsuma app)".into(),
            })
        }
    }
    Ok(Vec::new())
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
    let writers: Vec<&ProtocolObject<dyn NSPasteboardWriting>> =
        urls.iter().map(|u| ProtocolObject::from_ref(&**u)).collect();
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    if pb.writeObjects(&NSArray::from_slice(&writers)) {
        Ok(())
    } else {
        Err(anyhow::anyhow!("could not write to the clipboard").into())
    }
}

#[cfg(not(target_os = "macos"))]
fn copy_text(_: &str) -> Result<()> {
    Err(Error::MissingTool("a clipboard", "Only macOS is supported for now."))
}

#[cfg(not(target_os = "macos"))]
fn copy_files(_: &[PathBuf]) -> Result<()> {
    copy_text("")
}
