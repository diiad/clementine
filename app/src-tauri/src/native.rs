//! Small bits of AppKit that Tauri doesn't expose.

use std::path::PathBuf;

/// Float the window above everything (menus included), on every Space and
/// over full-screen apps, without a shadow. Must run on the main thread.
#[cfg(target_os = "macos")]
pub fn make_overlay(window: &tauri::WebviewWindow) {
    use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
    let Ok(ptr) = window.ns_window() else { return };
    let ns: &NSWindow = unsafe { &*(ptr as *const NSWindow) };
    // NSPopUpMenuWindowLevel
    ns.setLevel(101);
    ns.setHasShadow(false);
    ns.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
}

#[cfg(not(target_os = "macos"))]
pub fn make_overlay(_: &tauri::WebviewWindow) {}

/// wry always answers drags with `NSDragOperationCopy`. While ⌘ is held,
/// Finder only offers `Move`, so the drop would be refused, and ⌘ is the
/// default Tools key. Wrap wry's `draggingEntered:` / `draggingUpdated:` to
/// pick an operation the source allows. Clémentine never deletes the source, so
/// answering `Move` is harmless: for file drags the destination does the work.
#[cfg(target_os = "macos")]
pub fn accept_any_drag_operation() {
    use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
    use objc2::{msg_send, sel};
    use std::sync::OnceLock;

    type Handler = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize;
    static ENTERED: OnceLock<Handler> = OnceLock::new();
    static UPDATED: OnceLock<Handler> = OnceLock::new();

    unsafe fn adjust(op: usize, info: *mut AnyObject) -> usize {
        if op == 0 || info.is_null() {
            return op;
        }
        let mask: usize = msg_send![&*info, draggingSourceOperationMask];
        if op & mask != 0 {
            return op;
        }
        // Copy, Generic, Move, Link
        [1usize, 4, 16, 2]
            .into_iter()
            .find(|c| mask & c != 0)
            .unwrap_or(0)
    }
    unsafe extern "C-unwind" fn entered(
        this: *mut AnyObject,
        sel: Sel,
        info: *mut AnyObject,
    ) -> usize {
        adjust(
            ENTERED.get().expect("set before swap")(this, sel, info),
            info,
        )
    }
    unsafe extern "C-unwind" fn updated(
        this: *mut AnyObject,
        sel: Sel,
        info: *mut AnyObject,
    ) -> usize {
        adjust(
            UPDATED.get().expect("set before swap")(this, sel, info),
            info,
        )
    }

    let Some(class) = AnyClass::get(c"WryWebView") else {
        return;
    };
    for (sel, slot, ours) in [
        (sel!(draggingEntered:), &ENTERED, entered as Handler),
        (sel!(draggingUpdated:), &UPDATED, updated as Handler),
    ] {
        let Some(method) = class.instance_method(sel) else {
            continue;
        };
        if slot.get().is_some() {
            continue;
        }
        unsafe {
            let original: Handler = std::mem::transmute::<Imp, Handler>(method.implementation());
            let _ = slot.set(original);
            method.set_implementation(std::mem::transmute::<Handler, Imp>(ours));
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn accept_any_drag_operation() {}

/// Open the AirDrop sheet for `paths`. Must run on the main thread.
#[cfg(target_os = "macos")]
pub fn airdrop(paths: &[PathBuf]) -> anyhow::Result<()> {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSSharingService, NSSharingServiceNameSendViaAirDrop};
    use objc2_foundation::{NSArray, NSString, NSURL};
    let urls: Vec<Retained<NSURL>> = paths
        .iter()
        .map(|p| NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy())))
        .collect();
    let objs: Vec<&AnyObject> = urls.iter().map(|u| u.as_ref() as &AnyObject).collect();
    let items = NSArray::from_slice(&objs);
    let service =
        NSSharingService::sharingServiceNamed(unsafe { NSSharingServiceNameSendViaAirDrop })
            .ok_or_else(|| anyhow::anyhow!("AirDrop is not available on this Mac"))?;
    unsafe {
        if !service.canPerformWithItems(Some(&items)) {
            anyhow::bail!("AirDrop can't send these items");
        }
        service.performWithItems(&items);
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn airdrop(_: &[PathBuf]) -> anyhow::Result<()> {
    anyhow::bail!("AirDrop is macOS only")
}

/// The files selected in the frontmost Finder window (asks for Automation
/// permission the first time).
pub fn finder_selection() -> Vec<PathBuf> {
    const SCRIPT: &str = r#"
        set out to ""
        tell application "Finder"
            repeat with f in (get selection)
                set out to out & POSIX path of (f as alias) & linefeed
            end repeat
        end tell
        return out"#;
    std::process::Command::new("/usr/bin/osascript")
        .args(["-e", SCRIPT])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(|l| PathBuf::from(l.trim_end_matches('/')))
                .filter(|p| p.exists())
                .collect()
        })
        .unwrap_or_default()
}

/// Move files into `dir`, falling back to copy + delete across volumes.
pub fn move_into(paths: &[PathBuf], dir: &std::path::Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for p in paths {
        let dest = clementine_core::unique_path(&dir.join(p.file_name().unwrap_or_default()));
        if std::fs::rename(p, &dest).is_err() {
            // Different volume: let `ditto` copy (keeps metadata), then remove.
            let ok = std::process::Command::new("/usr/bin/ditto")
                .arg(p)
                .arg(&dest)
                .status()?
                .success();
            anyhow::ensure!(ok, "could not move {}", p.display());
            if p.is_dir() {
                std::fs::remove_dir_all(p)?;
            } else {
                std::fs::remove_file(p)?;
            }
        }
        out.push(dest);
    }
    Ok(out)
}
