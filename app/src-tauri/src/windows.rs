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
