//! Detects "a file is being dragged and a trigger key is held", system-wide.
//!
//! No Accessibility permission is needed: we poll the mouse buttons and
//! modifier flags (`+[NSEvent pressedMouseButtons]`, `+[NSEvent modifierFlags]`)
//! and watch the drag pasteboard, whose change count bumps whenever any app
//! starts a drag session.
//!
//! Windows has no drag pasteboard to read. We watch for a left-button drag
//! that starts in File Explorer or on the desktop, report the keys with no
//! paths, and the wheel's drop target picks the files up when the drag
//! enters it.

use std::path::PathBuf;
#[cfg(any(target_os = "macos", windows))]
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
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
pub enum Event {
    /// A file drag is in progress and the modifiers changed. On Windows the
    /// paths are not known yet and `paths` is empty.
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

#[cfg(windows)]
mod win {
    use super::*;
    use windows_sys::Win32::Foundation::{CloseHandle, POINT};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_LBUTTON, VK_LWIN, VK_MENU, VK_RBUTTON, VK_RWIN, VK_SHIFT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetAncestor, GetCursorPos, GetSystemMetrics, GetWindowThreadProcessId, WindowFromPoint,
        GA_ROOT, SM_CXDRAG, SM_CYDRAG, SM_SWAPBUTTON,
    };

    fn pressed(vk: u16) -> bool {
        unsafe { GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 }
    }

    /// The primary button, which is the right one for left-handed setups.
    pub fn mouse_down() -> bool {
        let swapped = unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0;
        pressed(if swapped { VK_RBUTTON } else { VK_LBUTTON })
    }

    /// ⌘ maps to the Windows key and ⌥ to Alt, as on Mac keyboards.
    pub fn mods() -> Mods {
        Mods {
            shift: pressed(VK_SHIFT),
            command: pressed(VK_LWIN) || pressed(VK_RWIN),
            control: pressed(VK_CONTROL),
            option: pressed(VK_MENU),
        }
    }

    pub fn cursor() -> (i32, i32) {
        let mut p = POINT { x: 0, y: 0 };
        unsafe { GetCursorPos(&mut p) };
        (p.x, p.y)
    }

    /// How far the pointer must travel before Windows calls it a drag.
    pub fn drag_threshold() -> (i32, i32) {
        unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) }
    }

    /// Whether the window under the point belongs to explorer.exe, which runs
    /// both File Explorer windows and the desktop.
    pub fn explorer_at((x, y): (i32, i32)) -> bool {
        unsafe {
            let hwnd = GetAncestor(WindowFromPoint(POINT { x, y }), GA_ROOT);
            if hwnd.is_null() {
                return false;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return false;
            }
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut len) != 0;
            CloseHandle(process);
            ok && String::from_utf16_lossy(&buf[..len as usize])
                .to_ascii_lowercase()
                .ends_with("\\explorer.exe")
        }
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
    #[cfg(windows)]
    std::thread::Builder::new()
        .name("drag-watcher".into())
        .spawn(move || {
            let mut was_down = false;
            // Some(start) while the button is held after pressing it in Explorer.
            let mut origin: Option<(i32, i32)> = None;
            let mut dragging = false;
            let mut last_mods = (false, false, false, false);
            loop {
                let down = win::mouse_down();
                if down && !was_down {
                    let at = win::cursor();
                    origin = win::explorer_at(at).then_some(at);
                    dragging = false;
                }
                if down {
                    if let (false, Some((x0, y0))) = (dragging, origin) {
                        let (x, y) = win::cursor();
                        let (dx, dy) = win::drag_threshold();
                        dragging = (x - x0).abs() > dx || (y - y0).abs() > dy;
                    }
                    if dragging {
                        let m = win::mods();
                        let key = (m.shift, m.command, m.control, m.option);
                        if key != last_mods {
                            last_mods = key;
                            on_event(Event::Drag {
                                paths: Vec::new(),
                                mods: m,
                            });
                        }
                    }
                } else if was_down {
                    if std::mem::take(&mut dragging) {
                        on_event(Event::Ended);
                    }
                    origin = None;
                    last_mods = (false, false, false, false);
                }
                was_down = down;
                std::thread::sleep(Duration::from_millis(if down { 16 } else { 40 }));
            }
        })
        .expect("spawn drag watcher");
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = on_event;
}
