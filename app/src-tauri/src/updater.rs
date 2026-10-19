//! Updates from the GitHub releases: a signed feed per platform
//! (`updater/<target>-<arch>.json`, written by the release workflow).

use crate::i18n::tr;
use crate::{lang, state};
use std::time::Duration;
use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tauri_plugin_updater::UpdaterExt;

const EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// Check shortly after launch, then every few hours, while enabled.
pub fn spawn_auto_check(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(30));
        loop {
            if state(&app).settings.lock().unwrap().auto_update {
                let _ = tauri::async_runtime::block_on(check(&app));
            }
            std::thread::sleep(EVERY);
        }
    });
}

/// Look for an update and, if there is one, offer to install it. Returns
/// the new version, or None when up to date.
pub async fn check(app: &AppHandle) -> Result<Option<String>, String> {
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    let Some(update) = update else {
        return Ok(None);
    };
    let version = update.version.clone();
    let l = lang(app);
    let message = tr(
        l,
        "Clémentine {v} is available. Install it now? The app restarts afterwards.",
    )
    .replace("{v}", &version);
    let handle = app.clone();
    app.dialog()
        .message(message)
        .title("Clémentine")
        .buttons(MessageDialogButtons::OkCancelCustom(
            tr(l, "Install").into(),
            tr(l, "Later").into(),
        ))
        .show(move |install| {
            if !install {
                return;
            }
            tauri::async_runtime::spawn(async move {
                match update.download_and_install(|_, _| {}, || {}).await {
                    Ok(()) => handle.restart(),
                    Err(e) => {
                        let l = lang(&handle);
                        handle
                            .dialog()
                            .message(format!("{} {e}", tr(l, "The update failed:")))
                            .title("Clémentine")
                            .show(|_| {});
                    }
                }
            });
        });
    Ok(Some(version))
}
