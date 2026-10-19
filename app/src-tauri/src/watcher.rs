//! Detects "a file is being dragged and a trigger key is held", system-wide.
//!
//! No Accessibility permission is needed: we poll the mouse buttons and
//! modifier flags (`+[NSEvent pressedMouseButtons]`, `+[NSEvent modifierFlags]`)
//! and watch the drag pasteboard, whose change count bumps whenever any app
//! starts a drag session.

use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modifier {
    Shift,
    Command,
    Control,
    Option,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Mods {
    pub shift: bool,
    pub command: bool,
    pub control: bool,
    pub option: bool,
}

impl Mods {
    pub fn has(&self, m: Modifier) -> bool {
        match m {
            Modifier::Shift => self.shift,
            Modifier::Command => self.command,
            Modifier::Control => self.control,
            Modifier::Option => self.option,
        }
    }
}

#[derive(Debug)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum Event {
    /// A file drag is in progress and the modifiers changed.
    Drag { paths: Vec<PathBuf>, mods: Mods },
    /// The mouse button was released (drop happened somewhere, or cancelled).
    Ended,
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use objc2_app_kit::{
        NSEvent, NSEventModifierFlags, NSPasteboard, NSPasteboardNameDrag, NSPasteboardTypeFileURL,
    };
    use objc2_foundation::{NSString, NSURL};

    pub fn mouse_down() -> bool {
        NSEvent::pressedMouseButtons() & 1 == 1
    }

    pub fn mods() -> Mods {
        let f = NSEvent::modifierFlags_class();
        Mods {
            shift: f.contains(NSEventModifierFlags::Shift),
            command: f.contains(NSEventModifierFlags::Command),
            control: f.contains(NSEventModifierFlags::Control),
            option: f.contains(NSEventModifierFlags::Option),
        }
    }

    pub fn drag_change_count() -> isize {
        NSPasteboard::pasteboardWithName(unsafe { NSPasteboardNameDrag }).changeCount()
    }

    /// File paths on the drag pasteboard (empty when dragging text, links…).
    pub fn dragged_files() -> Vec<PathBuf> {
        let pb = NSPasteboard::pasteboardWithName(unsafe { NSPasteboardNameDrag });
        let Some(items) = pb.pasteboardItems() else {
            return Vec::new();
        };
        let file_url = unsafe { NSPasteboardTypeFileURL };
        items
            .iter()
            .filter_map(|item| item.stringForType(file_url))
            .filter_map(|s: objc2::rc::Retained<NSString>| NSURL::URLWithString(&s))
            .filter_map(|url| {
                // Resolve file reference URLs (file:///.file/id=…) to real paths.
                let url = url.filePathURL().unwrap_or(url);
                url.path().map(|p| PathBuf::from(p.to_string()))
            })
            .collect()
    }
}

/// Spawn the polling thread. `on_event` is called from that thread.
pub fn spawn(on_event: impl Fn(Event) + Send + 'static) {
    #[cfg(target_os = "macos")]
    std::thread::Builder::new()
        .name("drag-watcher".into())
        .spawn(move || {
            let mut was_down = false;
            let mut baseline = mac::drag_change_count();
            let mut dragging_files: Option<Vec<PathBuf>> = None;
            let mut last_mods = (false, false, false, false);
            loop {
                let down = mac::mouse_down();
                if down && !was_down {
                    baseline = mac::drag_change_count();
                    dragging_files = None;
                }
                if down {
                    // The pasteboard can be filled a moment after the count bumps: retry while empty.
                    let have_files = dragging_files.as_ref().is_some_and(|f| !f.is_empty());
                    if !have_files && mac::drag_change_count() != baseline {
                        dragging_files = Some(mac::dragged_files());
                    }
                    if let Some(files) = dragging_files.as_ref().filter(|f| !f.is_empty()) {
                        let m = mac::mods();
                        let key = (m.shift, m.command, m.control, m.option);
                        if key != last_mods {
                            last_mods = key;
                            on_event(Event::Drag {
                                paths: files.clone(),
                                mods: m,
                            });
                        }
                    }
                } else if was_down {
                    if dragging_files.take().is_some_and(|f| !f.is_empty()) {
                        on_event(Event::Ended);
                    }
                    last_mods = (false, false, false, false);
                }
                was_down = down;
                std::thread::sleep(Duration::from_millis(if down { 16 } else { 40 }));
            }
        })
        .expect("spawn drag watcher");
    #[cfg(not(target_os = "macos"))]
    let _ = on_event;
}
