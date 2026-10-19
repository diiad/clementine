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

/// Centre the window on the mouse, kept whole on the screen under it.
///
/// Done in AppKit points: Tauri's physical cursor coordinates don't line up
/// across displays with different scale factors (Retina + non-Retina), which
/// put the wheel on the wrong screen. Must run on the main thread.
#[cfg(target_os = "macos")]
pub fn center_on_cursor(window: &tauri::WebviewWindow, size: f64) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSScreen, NSWindow};
    use objc2_foundation::NSPoint;
    let (Ok(ptr), Some(mtm)) = (window.ns_window(), MainThreadMarker::new()) else {
        return;
    };
    let ns: &NSWindow = unsafe { &*(ptr as *const NSWindow) };
    let mouse = NSEvent::mouseLocation();
    let half = size / 2.0;
    let (mut x, mut y) = (mouse.x - half, mouse.y - half);
    let screens = NSScreen::screens(mtm);
    let screen = screens.iter().find(|s| {
        let f = s.frame();
        mouse.x >= f.origin.x
            && mouse.x <= f.origin.x + f.size.width
            && mouse.y >= f.origin.y
            && mouse.y <= f.origin.y + f.size.height
    });
    if let Some(screen) = screen {
        let f = screen.frame();
        x = x.clamp(f.origin.x, f.origin.x + f.size.width - size);
        y = y.clamp(f.origin.y, f.origin.y + f.size.height - size);
    }
    ns.setFrameOrigin(NSPoint::new(x, y));
}

#[cfg(not(target_os = "macos"))]
pub fn center_on_cursor(window: &tauri::WebviewWindow, size: f64) {
    let Ok(c) = window.cursor_position() else {
        return;
    };
    let scale = window
        .monitor_from_point(c.x, c.y)
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);
    let half = size * scale / 2.0;
    let _ = window.set_position(tauri::PhysicalPosition::new(c.x - half, c.y - half));
}

/// wry always answers drags with `NSDragOperationCopy`. While ⌘ is held,
/// Finder only offers `Move`, so the drop would be refused, and ⌘ is the
/// default Tools key. Wrap wry's `draggingEntered:` / `draggingUpdated:` to
/// pick an operation the source allows. Harmless ones come first; `Move` is
/// the last resort (Finder with ⌘ held offers nothing else) and Clémentine
/// never deletes the source itself.
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
        // Copy, Generic, Link, Move
        [1usize, 4, 2, 16]
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

/// A tick of the trackpad. `Light` uses the public AppKit API, which macOS
/// only plays while a finger is on a Force Touch trackpad (the case mid-drag)
/// and whose strength can't be changed. `Medium` and `Strong` drive the
/// trackpad actuator through the private MultitouchSupport framework, like
/// HapticKey does, and fall back to AppKit when it isn't available.
#[cfg(target_os = "macos")]
pub fn haptic(strength: crate::settings::HapticStrength) {
    use crate::settings::HapticStrength;
    let actuation = match strength {
        HapticStrength::Light => None,
        HapticStrength::Medium => Some(4),
        HapticStrength::Strong => Some(6),
    };
    if let Some(id) = actuation {
        if actuator::actuate(id) {
            return;
        }
    }
    use objc2_app_kit::{
        NSHapticFeedbackManager, NSHapticFeedbackPattern, NSHapticFeedbackPerformanceTime,
        NSHapticFeedbackPerformer,
    };
    let pattern = match strength {
        HapticStrength::Light => NSHapticFeedbackPattern::Alignment,
        _ => NSHapticFeedbackPattern::Generic,
    };
    NSHapticFeedbackManager::defaultPerformer()
        .performFeedbackPattern_performanceTime(pattern, NSHapticFeedbackPerformanceTime::Now);
}

#[cfg(not(target_os = "macos"))]
pub fn haptic(_: crate::settings::HapticStrength) {}

/// The built-in trackpad's actuator, opened once through dlopen so a
/// missing or changed private framework just disables this path.
#[cfg(target_os = "macos")]
mod actuator {
    use std::ffi::{c_void, CStr};
    use std::sync::OnceLock;

    type CreateList = unsafe extern "C" fn() -> *const c_void;
    type GetDeviceId = unsafe extern "C" fn(*const c_void, *mut u64) -> i32;
    type CreateActuator = unsafe extern "C" fn(u64) -> *const c_void;
    type Open = unsafe extern "C" fn(*const c_void) -> i32;
    type Actuate = unsafe extern "C" fn(*const c_void, i32, u32, f32, f32) -> i32;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFArrayGetCount(array: *const c_void) -> isize;
        fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
    }

    struct Actuator {
        handle: *const c_void,
        actuate: Actuate,
    }
    // The actuator is only a CF reference used for fire-and-forget calls.
    unsafe impl Send for Actuator {}
    unsafe impl Sync for Actuator {}

    static ACTUATOR: OnceLock<Option<Actuator>> = OnceLock::new();

    unsafe fn sym<T>(lib: *mut c_void, name: &CStr) -> Option<T> {
        let p = libc::dlsym(lib, name.as_ptr());
        (!p.is_null()).then(|| std::mem::transmute_copy(&p))
    }

    fn open() -> Option<Actuator> {
        unsafe {
            let lib = libc::dlopen(
                c"/System/Library/PrivateFrameworks/MultitouchSupport.framework/MultitouchSupport"
                    .as_ptr(),
                libc::RTLD_LAZY,
            );
            if lib.is_null() {
                return None;
            }
            let list: CreateList = sym(lib, c"MTDeviceCreateList")?;
            let device_id: GetDeviceId = sym(lib, c"MTDeviceGetDeviceID")?;
            let create: CreateActuator = sym(lib, c"MTActuatorCreateFromDeviceID")?;
            let open: Open = sym(lib, c"MTActuatorOpen")?;
            let actuate: Actuate = sym(lib, c"MTActuatorActuate")?;
            let devices = list();
            if devices.is_null() {
                return None;
            }
            for i in 0..CFArrayGetCount(devices) {
                let mut id = 0u64;
                if device_id(CFArrayGetValueAtIndex(devices, i), &mut id) != 0 {
                    continue;
                }
                let handle = create(id);
                if !handle.is_null() && open(handle) == 0 {
                    return Some(Actuator { handle, actuate });
                }
            }
            None
        }
    }

    /// Play actuation `id` (1 = faint … 6 = strong). False if unavailable.
    pub fn actuate(id: i32) -> bool {
        match ACTUATOR.get_or_init(open) {
            Some(a) => unsafe { (a.actuate)(a.handle, id, 0, 0.0, 2.0) == 0 },
            None => false,
        }
    }
}

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

/// The files selected in the frontmost Explorer window.
#[cfg(windows)]
pub fn finder_selection() -> Vec<PathBuf> {
    crate::windows::explorer_selection()
}

/// The files selected in the frontmost Finder window (asks for Automation
/// permission the first time).
#[cfg(not(windows))]
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

/// Copy a file or folder. On macOS `ditto` keeps metadata and resource forks.
fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        let ok = std::process::Command::new("/usr/bin/ditto")
            .arg(from)
            .arg(to)
            .status()?
            .success();
        anyhow::ensure!(ok, "could not copy {}", from.display());
    } else if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else {
        std::fs::copy(from, to)?;
    }
    Ok(())
}

/// Move files into `dir`, falling back to copy + delete across volumes.
pub fn move_into(paths: &[PathBuf], dir: &std::path::Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for p in paths {
        let dest = clementine_core::unique_path(&dir.join(p.file_name().unwrap_or_default()));
        if std::fs::rename(p, &dest).is_err() {
            // Different volume: copy, then remove the original.
            copy_tree(p, &dest)?;
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
