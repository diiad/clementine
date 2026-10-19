//! Windows integration. There is no system-wide drag pasteboard to watch, so
//! the wheel opens from the File Explorer selection (shortcut) or from an
//! "Open with Clémentine" entry in the right-click menu.

use std::path::PathBuf;

/// Files selected in the File Explorer window that has the focus.
pub fn explorer_selection() -> Vec<PathBuf> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    let hwnd = unsafe { GetForegroundWindow() } as isize;
    // Shell.Application lists the open Explorer windows; pick the frontmost.
    let script = format!(
        "$OutputEncoding = [Console]::OutputEncoding = [Text.Encoding]::UTF8\n\
         (New-Object -ComObject Shell.Application).Windows() |\n\
           Where-Object {{ $_.HWND -eq {hwnd} }} |\n\
           ForEach-Object {{ $_.Document.SelectedItems() }} |\n\
           ForEach-Object {{ $_.Path }}"
    );
    clementine_core::engines::actions::powershell(&script)
        .map(|out| {
            out.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(PathBuf::from)
                .filter(|p| p.exists())
                .collect()
        })
        .unwrap_or_default()
}

/// Add "Open with Clémentine" to the right-click menu of files and folders,
/// for the current user only (no admin rights). Re-run at every launch so the
/// path follows the app if it moves.
pub fn register_context_menu(label: &str) -> std::io::Result<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let exe = std::env::current_exe()?;
    let exe = exe.to_string_lossy();
    let classes = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(r"Software\Classes", winreg::enums::KEY_ALL_ACCESS)?;
    for base in [r"*\shell\Clementine", r"Directory\shell\Clementine"] {
        let (key, _) = classes.create_subkey(base)?;
        key.set_value("MUIVerb", &label)?;
        key.set_value("Icon", &format!("\"{exe}\",0"))?;
        // One process per selected file; the single-instance plugin gathers
        // them. "Player" lifts Explorer's 15-item limit.
        key.set_value("MultiSelectModel", &"Player")?;
        let (cmd, _) = key.create_subkey("command")?;
        cmd.set_value("", &format!("\"{exe}\" --wheel \"%1\""))?;
    }
    Ok(())
}

/// A file drag over the wheel, in screen pixels.
pub enum Drag {
    Enter(Vec<PathBuf>, (i32, i32)),
    Over((i32, i32)),
    Leave,
    Drop(Vec<PathBuf>, (i32, i32)),
}

/// The listener returns whether it took a drop.
pub type DragListener = std::rc::Rc<dyn Fn(Drag) -> bool>;

/// Our own OLE drop target for the wheel, in place of wry's. wry leaves the
/// drop effect at whatever the source allowed, which Explorer can read as
/// "moved" and then delete the originals; this one always answers "copy".
/// It also hands the drag to the shell's drop helper, which keeps drawing
/// the dragged files' ghost image over the wheel.
mod drop_target {
    use super::{Drag, DragListener};
    use std::cell::{Cell, RefCell};
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;
    use win::core::{implement, Ref, BOOL};
    use win::Win32::Foundation::{HWND, LPARAM, POINT, POINTL};
    use win::Win32::System::Com::{
        CoCreateInstance, IDataObject, CLSCTX_INPROC_SERVER, DVASPECT_CONTENT, FORMATETC,
        TYMED_HGLOBAL,
    };
    use win::Win32::System::Ole::{
        IDropTarget, IDropTarget_Impl, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
        CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE,
    };
    use win::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
    use win::Win32::UI::Shell::{CLSID_DragDropHelper, DragQueryFileW, IDropTargetHelper, HDROP};
    use win::Win32::UI::WindowsAndMessaging::EnumChildWindows;

    thread_local! {
        static TARGETS: RefCell<Vec<IDropTarget>> = const { RefCell::new(Vec::new()) };
    }

    /// The file paths in a drag, or None when it carries no files.
    fn files(data: Option<&IDataObject>) -> Option<Vec<PathBuf>> {
        let data = data?;
        let format = FORMATETC {
            cfFormat: CF_HDROP.0,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        unsafe {
            let mut medium = data.GetData(&format).ok()?;
            let hdrop = HDROP(medium.u.hGlobal.0);
            let mut out = Vec::new();
            for i in 0..DragQueryFileW(hdrop, u32::MAX, None) {
                let len = DragQueryFileW(hdrop, i, None) as usize;
                let mut buf = vec![0u16; len + 1];
                DragQueryFileW(hdrop, i, Some(&mut buf));
                out.push(OsString::from_wide(&buf[..len]).into());
            }
            ReleaseStgMedium(&mut medium);
            Some(out).filter(|f: &Vec<PathBuf>| !f.is_empty())
        }
    }

    #[implement(IDropTarget)]
    struct Target {
        listener: DragListener,
        /// The wheel's top-level window, for the drop helper.
        window: HWND,
        helper: Option<IDropTargetHelper>,
        /// The drag in progress carries files.
        valid: Cell<bool>,
    }

    fn point(pt: &POINTL) -> POINT {
        POINT { x: pt.x, y: pt.y }
    }

    impl Target {
        fn effect(&self) -> DROPEFFECT {
            if self.valid.get() {
                DROPEFFECT_COPY
            } else {
                DROPEFFECT_NONE
            }
        }
    }

    #[allow(non_snake_case)]
    impl IDropTarget_Impl for Target_Impl {
        fn DragEnter(
            &self,
            data: Ref<'_, IDataObject>,
            _keys: MODIFIERKEYS_FLAGS,
            pt: &POINTL,
            effect: *mut DROPEFFECT,
        ) -> win::core::Result<()> {
            let data = data.as_ref();
            let paths = files(data);
            self.valid.set(paths.is_some());
            if let Some(paths) = paths {
                (self.listener)(Drag::Enter(paths, (pt.x, pt.y)));
            }
            unsafe { *effect = self.effect() };
            if let (Some(helper), Some(data)) = (&self.helper, data) {
                let _ = unsafe { helper.DragEnter(self.window, data, &point(pt), *effect) };
            }
            Ok(())
        }

        fn DragOver(
            &self,
            _keys: MODIFIERKEYS_FLAGS,
            pt: &POINTL,
            effect: *mut DROPEFFECT,
        ) -> win::core::Result<()> {
            if self.valid.get() {
                (self.listener)(Drag::Over((pt.x, pt.y)));
            }
            unsafe { *effect = self.effect() };
            if let Some(helper) = &self.helper {
                let _ = unsafe { helper.DragOver(&point(pt), *effect) };
            }
            Ok(())
        }

        fn DragLeave(&self) -> win::core::Result<()> {
            if let Some(helper) = &self.helper {
                let _ = unsafe { helper.DragLeave() };
            }
            if self.valid.replace(false) {
                (self.listener)(Drag::Leave);
            }
            Ok(())
        }

        fn Drop(
            &self,
            data: Ref<'_, IDataObject>,
            _keys: MODIFIERKEYS_FLAGS,
            pt: &POINTL,
            effect: *mut DROPEFFECT,
        ) -> win::core::Result<()> {
            let data = data.as_ref();
            if let (Some(helper), Some(data)) = (&self.helper, data) {
                let _ = unsafe { helper.Drop(data, &point(pt), self.effect()) };
            }
            let taken = self.valid.replace(false)
                && (self.listener)(Drag::Drop(files(data).unwrap_or_default(), (pt.x, pt.y)));
            unsafe {
                *effect = if taken {
                    DROPEFFECT_COPY
                } else {
                    DROPEFFECT_NONE
                }
            };
            Ok(())
        }
    }

    /// Register on the window and on every child, where WebView2 puts its
    /// own target. Call on the main thread.
    pub fn install(hwnd: isize, listener: DragListener) {
        let root = HWND(hwnd as _);
        let mut hwnds = vec![root];
        unsafe extern "system" fn collect(child: HWND, lparam: LPARAM) -> BOOL {
            (*(lparam.0 as *mut Vec<HWND>)).push(child);
            true.into()
        }
        unsafe {
            let _ = EnumChildWindows(
                Some(root),
                Some(collect),
                LPARAM(&mut hwnds as *mut _ as isize),
            );
        }
        let helper: Option<IDropTargetHelper> =
            unsafe { CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER) }.ok();
        let targets = hwnds
            .into_iter()
            .filter_map(|h| {
                let target: IDropTarget = Target {
                    listener: listener.clone(),
                    window: root,
                    helper: helper.clone(),
                    valid: Cell::new(false),
                }
                .into();
                unsafe {
                    let _ = RevokeDragDrop(h);
                    RegisterDragDrop(h, &target).ok().map(|_| target)
                }
            })
            .collect();
        TARGETS.with(|t| *t.borrow_mut() = targets);
    }
}

pub use drop_target::install as install_drop_target;
