//! "Job finished" notifications with a button that opens the result. The
//! notification plugin has no actions on desktop, so these talk to the
//! system directly: NSUserNotification on macOS, toasts on Windows.

use crate::i18n::{tr, Lang};
use clementine_core::Options;
use std::path::PathBuf;
use tauri::AppHandle;

/// Open the results, or show them in the file manager.
fn act(op: &str, outputs: &[PathBuf]) {
    let _ = clementine_core::run(op, outputs, &Options::default(), &|_| {});
}

/// Show a notification for `outputs`: its Open button opens them, a click
/// elsewhere on it reveals them.
#[cfg(target_os = "macos")]
pub fn finished(app: &AppHandle, title: String, body: String, outputs: Vec<PathBuf>, lang: Lang) {
    use mac_notification_sys::{MainButton, Notification, NotificationResponse};
    // Same sender as the plugin's notifications (Terminal in development,
    // whose bundle macOS knows).
    let bundle = if tauri::is_dev() {
        "com.apple.Terminal".to_string()
    } else {
        app.config().identifier.clone()
    };
    // `send` waits for the user's answer.
    std::thread::spawn(move || {
        // Fails once already set, by us or the plugin: same bundle either way.
        let _ = mac_notification_sys::set_application(&bundle);
        let response = Notification::new()
            .title(&title)
            .message(&body)
            .main_button(MainButton::SingleAction(tr(lang, "Open")))
            .send();
        match response {
            Ok(NotificationResponse::ActionButton(_)) => act("action:open", &outputs),
            Ok(NotificationResponse::Click) => act("action:reveal", &outputs),
            _ => {}
        }
    });
}

#[cfg(windows)]
pub fn finished(app: &AppHandle, title: String, body: String, outputs: Vec<PathBuf>, lang: Lang) {
    use tauri_winrt_notification::Toast;
    // Toasts need the AppUserModelID the installer registered; a build run
    // from `target\` has none, so borrow PowerShell's, like the plugin.
    let installed = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.to_path_buf()))
        .is_some_and(|dir| !dir.ends_with("target\\debug") && !dir.ends_with("target\\release"));
    let id = if installed {
        app.config().identifier.clone()
    } else {
        Toast::POWERSHELL_APP_ID.to_string()
    };
    let _ = Toast::new(&id)
        .title(&title)
        .text1(&body)
        .add_button(tr(lang, "Open"), "open")
        .add_button(tr(lang, "Show in folder"), "reveal")
        .on_activated(move |action| {
            let op = match action.as_deref() {
                Some("open") => "action:open",
                _ => "action:reveal",
            };
            act(op, &outputs);
            Ok(())
        })
        .show();
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn finished(app: &AppHandle, title: String, body: String, _: Vec<PathBuf>, _: Lang) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title(title).body(body).show();
}
