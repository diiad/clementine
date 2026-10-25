//! Clémentine: a menu-bar app. Drag a file, hold a key, and a wheel of
//! conversions, tools or actions opens under the cursor; drop on a slice.

// A GUI app on Windows: no console window behind it (kept in debug builds
// for the logs).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod i18n;
mod install;
mod native;
mod notice;
mod settings;
mod updater;
mod watcher;
mod wheel;
#[cfg(windows)]
mod windows;

use clementine_core::{operations, Category, Operation};
use i18n::{tr, Lang};
use serde::Serialize;
use settings::Settings;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{
    AppHandle, DragDropEvent, Emitter, Manager, PhysicalPosition, RunEvent, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_notification::NotificationExt;
use wheel::{Hit, Segment};

#[derive(Default)]
struct Session {
    paths: Vec<PathBuf>,
    category: Option<Category>,
    ops: Vec<Operation>,
    segments: Vec<Segment>,
    visible: bool,
    busy: bool,
    /// Opened from the shortcut (click / keyboard) rather than by a drag.
    click_mode: bool,
}

struct AppState {
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    session: Mutex<Session>,
    floating: Mutex<Vec<PathBuf>>,
    /// The file open in the trim / crop editor.
    editing: Mutex<Option<PathBuf>>,
}

#[derive(Clone, Serialize)]
struct ShowPayload {
    /// Shown but not drawn yet: the files arrive with the drag (Windows).
    pending: bool,
    category: Category,
    title: String,
    segments: Vec<Segment>,
    click_mode: bool,
    hub: f64,
    size: f64,
    lang: Lang,
}

#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
enum Status {
    Working { label: String, progress: f32 },
    Done { message: String },
    Error { message: String },
}

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

/// Slice under the pointer during a drag (-1 none, -2 hub), to tick the
/// trackpad only when it changes.
static HOVER: AtomicI64 = AtomicI64::new(-1);

fn hover_changed(app: &AppHandle, index: i64) {
    if HOVER.swap(index, Ordering::Relaxed) == index || index < 0 {
        return;
    }
    let (on, strength) = {
        let st = state(app);
        let s = st.settings.lock().unwrap();
        (s.haptics, s.haptic_strength)
    };
    if on {
        native::haptic(strength);
    }
}

/// The system file manager, as named in menus and messages.
const OPEN_SELECTION: &str = if cfg!(windows) {
    "Open Wheel for Explorer Selection"
} else {
    "Open Wheel for Finder Selection"
};
const SELECT_FIRST: &str = if cfg!(windows) {
    "Select files in File Explorer first."
} else {
    "Select files in Finder first."
};

/// Files handed to the app on the command line (`--wheel`) or, on Windows,
/// by the right-click menu, which starts one process per selected file.
/// Gather what arrives within a short window into one selection.
static PENDING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static PENDING_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn queue_selection(app: &AppHandle, paths: Vec<PathBuf>) {
    PENDING.lock().unwrap().extend(paths);
    let generation = PENDING_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(350));
        if PENDING_GEN.load(Ordering::SeqCst) != generation {
            return; // more files arrived; the newest thread opens the wheel
        }
        let mut paths = std::mem::take(&mut *PENDING.lock().unwrap());
        paths.dedup();
        if paths.is_empty() {
            return;
        }
        let h = app.clone();
        let _ = app.run_on_main_thread(move || open_wheel(&h, paths, Category::Convert, true));
    });
}

/// Paths following `--wheel` in a command line.
fn wheel_args(args: &[String]) -> Option<Vec<PathBuf>> {
    let i = args.iter().position(|a| a == "--wheel")?;
    Some(
        args[i + 1..]
            .iter()
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .collect(),
    )
}

fn lang(app: &AppHandle) -> Lang {
    state(app).settings.lock().unwrap().language.resolve()
}

fn title_for(paths: &[PathBuf], lang: Lang) -> String {
    match paths {
        [one] => one
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        many => format!("{} {}", many.len(), tr(lang, "items")),
    }
}

// --- Wheel window ------------------------------------------------------------

fn wheel_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(w) = app.get_webview_window("wheel") {
        return Ok(w);
    }
    let w = WebviewWindowBuilder::new(app, "wheel", WebviewUrl::App("wheel.html".into()))
        .title("Clémentine")
        .inner_size(wheel::SIZE, wheel::SIZE)
        .transparent(true)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .shadow(false)
        .focused(false)
        .visible(false)
        .accept_first_mouse(true);
    // Windows uses our own drop target, installed when the wheel shows.
    #[cfg(windows)]
    let w = w.disable_drag_drop_handler();
    let w = w.build()?;
    native::make_overlay(&w);
    native::accept_any_drag_operation();
    let handle = app.clone();
    w.on_window_event(move |e| on_wheel_event(&handle, e));
    Ok(w)
}

/// Show (or refresh) the wheel. Must run on the main thread.
fn open_wheel(app: &AppHandle, paths: Vec<PathBuf>, category: Category, click_mode: bool) {
    let Ok(window) = wheel_window(app) else {
        return;
    };
    let st = state(app);
    let mut s = st.session.lock().unwrap();
    if s.busy {
        return;
    }
    let settings = st.settings.lock().unwrap();
    let lang = settings.language.resolve();
    let mut ops = operations(&paths, category);
    if category == Category::Tool {
        // Recipes come first: they are the user's own.
        let recipes = settings
            .recipes
            .iter()
            .enumerate()
            .filter(|(_, r)| r.applies(&paths))
            .map(|(i, r)| Operation {
                id: format!("recipe:{i}"),
                label: r.name.clone(),
                category,
            });
        ops.splice(0..0, recipes);
    }
    for op in &mut ops {
        op.label = i18n::op_label(lang, &op.id, &op.label);
        if op.id == "tool:fit-size" {
            op.label = i18n::fit_size_label(lang, settings.target_mb);
        }
    }
    drop(settings);
    let segments = wheel::layout(&ops);
    let payload = ShowPayload {
        pending: paths.is_empty() && !click_mode,
        lang,
        category,
        title: title_for(&paths, lang),
        segments: segments.clone(),
        click_mode,
        hub: wheel::HUB,
        size: wheel::SIZE,
    };
    let was_visible = s.visible;
    *s = Session {
        paths,
        category: Some(category),
        ops,
        segments,
        visible: true,
        busy: false,
        click_mode,
    };
    drop(s);
    if !was_visible {
        native::center_on_cursor(&window, wheel::SIZE);
        // WebView2 may have (re)created its child windows since last time.
        #[cfg(windows)]
        install_drop_target(app, &window);
    }
    HOVER.store(-1, Ordering::Relaxed);
    let _ = window.emit("wheel:show", payload);
    let _ = window.show();
    if click_mode {
        let _ = window.set_focus();
    }
}

fn hide_wheel(app: &AppHandle) {
    {
        let st = state(app);
        let mut s = st.session.lock().unwrap();
        s.visible = false;
        s.busy = false;
        s.click_mode = false;
    }
    if let Some(w) = app.get_webview_window("wheel") {
        let _ = w.emit("wheel:hide", ());
        let _ = w.hide();
    }
}

/// A file drag over the wheel, in logical window coordinates.
enum WheelDrag {
    Enter(Vec<PathBuf>, f64, f64),
    Over(f64, f64),
    Leave,
    Drop(Vec<PathBuf>, f64, f64),
}

fn on_wheel_event(app: &AppHandle, event: &WindowEvent) {
    let WindowEvent::DragDrop(dd) = event else {
        return;
    };
    let Some(window) = app.get_webview_window("wheel") else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(2.0);
    let local = |p: &PhysicalPosition<f64>| {
        // On macOS wry reports drag positions in points even though Tauri
        // types them as physical pixels.
        if cfg!(target_os = "macos") {
            (p.x, p.y)
        } else {
            let l = p.to_logical::<f64>(scale);
            (l.x, l.y)
        }
    };
    let drag = match dd {
        DragDropEvent::Enter { paths, position } => {
            let (x, y) = local(position);
            WheelDrag::Enter(paths.clone(), x, y)
        }
        DragDropEvent::Over { position } => {
            let (x, y) = local(position);
            WheelDrag::Over(x, y)
        }
        DragDropEvent::Leave => WheelDrag::Leave,
        DragDropEvent::Drop { paths, position } => {
            let (x, y) = local(position);
            WheelDrag::Drop(paths.clone(), x, y)
        }
        _ => return,
    };
    on_wheel_drag(app, drag);
}

/// Returns whether a drop was taken.
fn on_wheel_drag(app: &AppHandle, drag: WheelDrag) -> bool {
    let Some(window) = app.get_webview_window("wheel") else {
        return false;
    };
    let locate = |x: f64, y: f64| {
        let st = state(app);
        let s = st.session.lock().unwrap();
        wheel::hit(&s.segments, x, y)
    };
    let hover = |x: f64, y: f64| {
        let index = match locate(x, y) {
            Hit::Segment(i) => i as i64,
            Hit::Hub => -2,
            Hit::Outside => -1,
        };
        hover_changed(app, index);
        let _ = window.emit("wheel:hover", index);
    };
    match drag {
        WheelDrag::Enter(paths, x, y) => {
            // On Windows the wheel opens before it knows the files: fill it
            // in now that the drag brings them.
            let pending = {
                let st = state(app);
                let s = st.session.lock().unwrap();
                s.category
                    .filter(|_| s.visible && !s.busy && s.paths.is_empty())
            };
            if let (Some(category), false) = (pending, paths.is_empty()) {
                open_wheel(app, paths, category, false);
            }
            hover(x, y);
            false
        }
        WheelDrag::Over(x, y) => {
            hover(x, y);
            false
        }
        WheelDrag::Leave => {
            HOVER.store(-1, Ordering::Relaxed);
            let _ = window.emit("wheel:hover", -1);
            false
        }
        WheelDrag::Drop(paths, x, y) => match locate(x, y) {
            Hit::Segment(i) => {
                let paths = if paths.is_empty() {
                    state(app).session.lock().unwrap().paths.clone()
                } else {
                    paths
                };
                start_job(app, i, paths);
                true
            }
            _ => {
                hide_wheel(app);
                false
            }
        },
    }
}

/// Replace wry's drop target on the wheel with ours (see `windows.rs`).
#[cfg(windows)]
fn install_drop_target(app: &AppHandle, window: &WebviewWindow) {
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let handle = app.clone();
    windows::install_drop_target(
        hwnd.0 as isize,
        std::rc::Rc::new(move |drag| {
            let Some(w) = handle.get_webview_window("wheel") else {
                return false;
            };
            let scale = w.scale_factor().unwrap_or(1.0);
            let origin = w.inner_position().unwrap_or_default();
            let local =
                |(x, y): (i32, i32)| ((x - origin.x) as f64 / scale, (y - origin.y) as f64 / scale);
            let drag = match drag {
                windows::Drag::Enter(paths, p) => {
                    let (x, y) = local(p);
                    WheelDrag::Enter(paths, x, y)
                }
                windows::Drag::Over(p) => {
                    let (x, y) = local(p);
                    WheelDrag::Over(x, y)
                }
                windows::Drag::Leave => WheelDrag::Leave,
                windows::Drag::Drop(paths, p) => {
                    let (x, y) = local(p);
                    WheelDrag::Drop(paths, x, y)
                }
            };
            on_wheel_drag(&handle, drag)
        }),
    );
}

// --- Jobs ----------------------------------------------------------------------

fn emit_status(app: &AppHandle, status: Status) {
    let _ = app.emit_to("wheel", "wheel:status", status);
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    if state(app).settings.lock().unwrap().notify {
        let _ = app.notification().builder().title(title).body(body).show();
    }
}

/// A job finished: offer to open what it produced.
fn notify_finished(app: &AppHandle, title: String, body: String, outputs: &[PathBuf]) {
    if outputs.is_empty() {
        return notify(app, &title, &body);
    }
    if state(app).settings.lock().unwrap().notify {
        notice::finished(app, title, body, outputs.to_vec(), lang(app));
    }
}

/// Run the operation in slice `index` on `paths`. Called on the main thread.
fn start_job(app: &AppHandle, index: usize, paths: Vec<PathBuf>) {
    let lang = lang(app);
    let (op, opts, reveal, steps) = {
        let st = state(app);
        let mut s = st.session.lock().unwrap();
        let Some(op) = s.ops.get(index).cloned() else {
            return;
        };
        if s.busy {
            return;
        }
        s.busy = true;
        let settings = st.settings.lock().unwrap();
        let steps = op
            .id
            .strip_prefix("recipe:")
            .and_then(|i| settings.recipes.get(i.parse::<usize>().ok()?))
            .map(|r| r.steps.clone());
        (op, settings.options(), settings.reveal_after, steps)
    };

    // Actions that need the main thread or a UI.
    match op.id.as_str() {
        "action:airdrop" => {
            hide_wheel(app);
            if let Err(e) = native::airdrop(&paths) {
                notify(app, "AirDrop", &e.to_string());
            }
            return;
        }
        "action:float" => {
            hide_wheel(app);
            open_float(app, paths);
            return;
        }
        "tool:trim" | "tool:crop" => {
            hide_wheel(app);
            if let Some(path) = paths.into_iter().next() {
                open_editor(app, path);
            }
            return;
        }
        "action:move" => {
            hide_wheel(app);
            let app = app.clone();
            std::thread::spawn(move || {
                let Some(dir) = app
                    .dialog()
                    .file()
                    .set_title(tr(lang, "Move to…"))
                    .blocking_pick_folder()
                else {
                    return;
                };
                let Ok(dir) = dir.into_path() else { return };
                if let Err(e) = native::move_into(&paths, &dir) {
                    notify(&app, tr(lang, "Move failed"), &e.to_string());
                }
            });
            return;
        }
        _ => {}
    }

    emit_status(
        app,
        Status::Working {
            label: op.label.clone(),
            progress: 0.0,
        },
    );
    let app = app.clone();
    std::thread::spawn(move || {
        let started = Instant::now();
        let last = Mutex::new(-1i32);
        let progress = |p: f32| {
            let pct = (p * 100.0) as i32;
            let mut l = last.lock().unwrap();
            if pct != *l {
                *l = pct;
                emit_status(
                    &app,
                    Status::Working {
                        label: op.label.clone(),
                        progress: p,
                    },
                );
            }
        };
        let result = match &steps {
            Some(steps) => clementine_core::run_recipe(steps, &paths, &opts, &progress),
            None => clementine_core::run(&op.id, &paths, &opts, &progress),
        };
        let quick = op.id.starts_with("action:");
        let linger = match &result {
            Ok(outputs) => {
                let message = match outputs.as_slice() {
                    [] => tr(lang, "Done").to_string(),
                    [one] => one
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    many => format!("{} {}", many.len(), tr(lang, "files")),
                };
                emit_status(
                    &app,
                    Status::Done {
                        message: message.clone(),
                    },
                );
                if started.elapsed() > Duration::from_secs(5) {
                    notify_finished(
                        &app,
                        format!("{} {}", op.label, tr(lang, "finished")),
                        message,
                        outputs,
                    );
                }
                if reveal && !outputs.is_empty() {
                    let _ = clementine_core::run("action:reveal", outputs, &opts, &|_| {});
                }
                if quick {
                    250
                } else {
                    700
                }
            }
            Err(e) => {
                emit_status(
                    &app,
                    Status::Error {
                        message: e.to_string(),
                    },
                );
                notify(
                    &app,
                    &format!("{} {}", op.label, tr(lang, "failed")),
                    &e.to_string(),
                );
                3000
            }
        };
        std::thread::sleep(Duration::from_millis(linger));
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || hide_wheel(&handle));
    });
}

// --- Drag watcher ----------------------------------------------------------------

fn on_watch_event(app: &AppHandle, event: watcher::Event) {
    let handle = app.clone();
    match event {
        watcher::Event::Drag { paths, mods } => {
            let _ = app.run_on_main_thread(move || {
                let category = {
                    let st = state(&handle);
                    let settings = st.settings.lock().unwrap();
                    if !settings.enabled {
                        return;
                    }
                    settings.category_for(&mods)
                };
                let (visible, current, busy, known) = {
                    let st = state(&handle);
                    let s = st.session.lock().unwrap();
                    (s.visible, s.category, s.busy, s.paths.clone())
                };
                // Windows reports the keys before it knows the files.
                let paths = if paths.is_empty() && visible {
                    known
                } else {
                    paths
                };
                // Releasing the key keeps the wheel open, like Tangerine; a
                // different key switches wheels.
                if let Some(c) = category {
                    if !busy && (!visible || current != Some(c)) {
                        open_wheel(&handle, paths, c, false);
                    }
                }
            });
        }
        watcher::Event::Ended => {
            // Give the drop event, which arrives through the window, a head start.
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                let h = handle.clone();
                let _ = handle.run_on_main_thread(move || {
                    let hide = {
                        let st = state(&h);
                        let s = st.session.lock().unwrap();
                        s.visible && !s.busy && !s.click_mode
                    };
                    if hide {
                        hide_wheel(&h);
                    }
                });
            });
        }
    }
}

/// Shortcut / menu entry: open the wheel for the Finder selection.
fn open_for_selection(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let paths = native::finder_selection();
        let h = app.clone();
        let _ = app.run_on_main_thread(move || {
            if paths.is_empty() {
                notify(&h, "Clémentine", tr(lang(&h), SELECT_FIRST));
            } else {
                open_wheel(&h, paths, Category::Convert, true);
            }
        });
    });
}

// --- Float shelf ------------------------------------------------------------------

fn open_float(app: &AppHandle, paths: Vec<PathBuf>) {
    {
        let st = state(app);
        let mut f = st.floating.lock().unwrap();
        for p in paths {
            if !f.contains(&p) {
                f.push(p);
            }
        }
    }
    let window = match app.get_webview_window("float") {
        Some(w) => w,
        None => {
            let builder =
                WebviewWindowBuilder::new(app, "float", WebviewUrl::App("float.html".into()))
                    .title("Float")
                    .inner_size(260.0, 320.0)
                    .min_inner_size(200.0, 120.0)
                    .always_on_top(true)
                    .visible_on_all_workspaces(true)
                    .skip_taskbar(true);
            #[cfg(target_os = "macos")]
            let builder = builder
                .title_bar_style(tauri::TitleBarStyle::Overlay)
                .hidden_title(true);
            let Ok(w) = builder.build() else {
                return;
            };
            w
        }
    };
    let _ = window.emit("float:update", ());
    let _ = window.show();
}

// --- Trim / crop editor ----------------------------------------------------------

fn open_editor(app: &AppHandle, path: PathBuf) {
    // The page shows the file through the asset protocol: allow just this one.
    if let Err(e) = app.asset_protocol_scope().allow_file(&path) {
        eprintln!("could not allow {} for the editor: {e}", path.display());
    }
    *state(app).editing.lock().unwrap() = Some(path.clone());
    let title = title_for(&[path], lang(app));
    if let Some(w) = app.get_webview_window("editor") {
        let _ = w.set_title(&title);
        let _ = w.emit("editor:open", ());
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let builder = WebviewWindowBuilder::new(app, "editor", WebviewUrl::App("editor.html".into()))
        .title(&title)
        .inner_size(760.0, 600.0)
        .min_inner_size(480.0, 400.0)
        .center()
        .focused(true);
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    if let Ok(w) = builder.build() {
        let _ = w.set_focus();
    }
}

fn close_editor(app: &AppHandle) {
    *state(app).editing.lock().unwrap() = None;
    if let Some(w) = app.get_webview_window("editor") {
        let _ = w.close();
    }
}

#[derive(Serialize)]
struct EditorFile {
    path: String,
    name: String,
    kind: clementine_core::Kind,
    /// Seconds, for audio and video (the page can't always play the file).
    duration: Option<f64>,
}

#[tauri::command]
fn editor_file(app: AppHandle) -> Option<EditorFile> {
    let path = state(&app).editing.lock().unwrap().clone()?;
    let kind = clementine_core::detect(&path);
    let duration = matches!(
        kind,
        clementine_core::Kind::Audio | clementine_core::Kind::Video
    )
    .then(|| clementine_core::engines::media::duration(&path))
    .flatten();
    Some(EditorFile {
        name: title_for(std::slice::from_ref(&path), lang(&app)),
        path: path.display().to_string(),
        kind,
        duration,
    })
}

#[tauri::command]
fn editor_close(app: AppHandle) {
    close_editor(&app);
}

/// Run the trim or crop the page settled on. Progress and failures go back
/// to the page; on success the window closes.
#[tauri::command]
fn editor_apply(app: AppHandle, operation: String) -> Result<(), String> {
    if !(operation.starts_with("tool:trim=") || operation.starts_with("tool:crop=")) {
        return Err(format!("unexpected operation {operation}"));
    }
    let path = state(&app)
        .editing
        .lock()
        .unwrap()
        .clone()
        .ok_or("no file open")?;
    let (opts, reveal) = {
        let st = state(&app);
        let s = st.settings.lock().unwrap();
        (s.options(), s.reveal_after)
    };
    std::thread::spawn(move || {
        let started = Instant::now();
        let progress = |p: f32| {
            let _ = app.emit_to("editor", "editor:progress", p);
        };
        match clementine_core::run(&operation, &[path], &opts, &progress) {
            Ok(outputs) => {
                let h = app.clone();
                let _ = app.run_on_main_thread(move || close_editor(&h));
                if reveal && !outputs.is_empty() {
                    let _ = clementine_core::run("action:reveal", &outputs, &opts, &|_| {});
                }
                if started.elapsed() > Duration::from_secs(5) {
                    let l = lang(&app);
                    let name = outputs
                        .first()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    notify_finished(&app, tr(l, "Done").to_string(), name, &outputs);
                }
            }
            Err(e) => {
                let _ = app.emit_to("editor", "editor:failed", e.to_string());
            }
        }
    });
    Ok(())
}

// --- Settings window -------------------------------------------------------------

fn open_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    if let Ok(w) =
        WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
            .title(tr(lang(app), "Clémentine Settings"))
            .inner_size(520.0, 860.0)
            .resizable(false)
            .center()
            .build()
    {
        let _ = w.set_focus();
    }
}

fn register_shortcut(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    if shortcut.trim().is_empty() {
        return Ok(());
    }
    gs.on_shortcut(shortcut, |app, _, event| {
        if event.state() == ShortcutState::Pressed {
            open_for_selection(app);
        }
    })
    .map_err(|e| e.to_string())
}

// --- Commands ----------------------------------------------------------------------

#[tauri::command]
fn wheel_pick(app: AppHandle, index: usize) {
    let paths = state(&app).session.lock().unwrap().paths.clone();
    start_job(&app, index, paths);
}

#[tauri::command]
fn wheel_category(app: AppHandle, category: Category) {
    let (paths, click) = {
        let st = state(&app);
        let s = st.session.lock().unwrap();
        (s.paths.clone(), s.click_mode)
    };
    if !paths.is_empty() {
        open_wheel(&app, paths, category, click);
    }
}

/// Settings: let the user feel a strength right away.
#[tauri::command]
fn test_haptic(strength: settings::HapticStrength) {
    native::haptic(strength);
}

/// Click mode: the page moved the highlight with the mouse or keyboard.
#[tauri::command]
fn wheel_hover(app: AppHandle, index: i64) {
    hover_changed(&app, index);
}

#[tauri::command]
fn wheel_close(app: AppHandle) {
    if !state(&app).session.lock().unwrap().busy {
        hide_wheel(&app);
    }
}

#[derive(Serialize)]
struct CatalogGroup {
    family: &'static str,
    ops: Vec<Operation>,
}

/// Settings: the operations recipes can be built from.
#[tauri::command]
fn operation_catalog(app: AppHandle) -> Vec<CatalogGroup> {
    let (lang, target_mb) = {
        let st = state(&app);
        let s = st.settings.lock().unwrap();
        (s.language.resolve(), s.target_mb)
    };
    clementine_core::catalog()
        .into_iter()
        .map(|(family, mut ops)| {
            for op in &mut ops {
                op.label = i18n::op_label(lang, &op.id, &op.label);
                if op.id == "tool:fit-size" {
                    op.label = i18n::fit_size_label(lang, target_mb);
                }
            }
            CatalogGroup { family, ops }
        })
        .collect()
}

#[tauri::command]
fn ui_language(app: AppHandle) -> Lang {
    lang(&app)
}

#[tauri::command]
fn get_settings(app: AppHandle) -> Settings {
    state(&app).settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    let st = state(&app);
    let previous = st.settings.lock().unwrap().shortcut.clone();
    if previous != settings.shortcut {
        // Another app may own the combination: keep the old one then.
        if let Err(e) = register_shortcut(&app, &settings.shortcut) {
            let _ = register_shortcut(&app, &previous);
            return Err(format!(
                "{}: {e}",
                tr(settings.language.resolve(), "This shortcut can't be used")
            ));
        }
    }
    settings
        .save(&st.settings_path)
        .map_err(|e| e.to_string())?;
    let language_changed = st.settings.lock().unwrap().language != settings.language;
    *st.settings.lock().unwrap() = settings.clone();
    apply_tray_visibility(&app);
    if language_changed {
        translate_tray(&app);
        if let Some(w) = app.get_webview_window("settings") {
            let _ = w.set_title(tr(settings.language.resolve(), "Clémentine Settings"));
        }
    }
    if let Some(tray) = app.try_state::<TrayItems>() {
        let _ = tray.toggle.set_checked(settings.enabled);
    }
    Ok(())
}

#[tauri::command]
async fn choose_folder(app: AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .blocking_pick_folder()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.display().to_string())
}

#[derive(Serialize)]
struct Engines {
    version: String,
    /// `macos` or `windows`, to adapt the Settings page.
    platform: &'static str,
    ffmpeg: Option<String>,
    /// The ffmpeg shipped inside the app, rather than an installed one.
    ffmpeg_bundled: bool,
    libreoffice: Option<String>,
    autostart: bool,
}

#[tauri::command]
fn engine_status(app: AppHandle) -> Engines {
    let ffmpeg = clementine_core::engines::find_tool("ffmpeg");
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.to_path_buf()));
    Engines {
        platform: std::env::consts::OS,
        version: app.package_info().version.to_string(),
        ffmpeg_bundled: ffmpeg.as_ref().and_then(|p| p.parent()) == exe_dir.as_deref(),
        ffmpeg: ffmpeg.map(|p| p.display().to_string()),
        libreoffice: clementine_core::engines::document::office().map(|p| p.display().to_string()),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
    }
}

/// "Check for updates" in Settings: the new version, or None if up to date.
#[tauri::command]
async fn check_update(app: AppHandle) -> Result<Option<String>, String> {
    updater::check(&app).await
}

/// Relaunch, for an engine that only shows up in a fresh process.
#[tauri::command]
fn restart_app(app: AppHandle) {
    app.restart();
}

/// Install ffmpeg or LibreOffice from the Settings page.
#[tauri::command]
async fn install_engine(name: String) -> Result<install::Outcome, String> {
    let engine = install::Engine::parse(&name).ok_or("unknown engine")?;
    tauri::async_runtime::spawn_blocking(move || install::install(engine))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let al = app.autolaunch();
    if enabled { al.enable() } else { al.disable() }.map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct FloatItem {
    path: String,
    name: String,
    kind: clementine_core::Kind,
}

#[tauri::command]
fn float_items(app: AppHandle) -> Vec<FloatItem> {
    let st = state(&app);
    let mut f = st.floating.lock().unwrap();
    f.retain(|p| p.exists());
    f.iter()
        .map(|p| FloatItem {
            path: p.display().to_string(),
            name: p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            kind: clementine_core::detect(p),
        })
        .collect()
}

/// Start dragging floated files out of the shelf. Sync command → main thread.
#[tauri::command]
fn float_drag(app: AppHandle, window: WebviewWindow, index: Option<usize>) -> Result<(), String> {
    let all = state(&app).floating.lock().unwrap().clone();
    let paths = match index {
        Some(i) => all.get(i).cloned().into_iter().collect(),
        None => all,
    };
    if paths.is_empty() {
        return Ok(());
    }
    let icon = drag::Image::Raw(include_bytes!("../icons/64x64.png").to_vec());
    drag::start_drag(
        &window,
        drag::DragItem::Files(paths),
        icon,
        |_, _| {},
        drag::Options::default(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn float_remove(app: AppHandle, index: Option<usize>) {
    let st = state(&app);
    let mut f = st.floating.lock().unwrap();
    match index {
        Some(i) if i < f.len() => {
            f.remove(i);
        }
        Some(_) => {}
        None => f.clear(),
    }
}

#[tauri::command]
fn float_open_wheel(app: AppHandle, index: usize) {
    let path = state(&app).floating.lock().unwrap().get(index).cloned();
    if let Some(p) = path {
        open_wheel(&app, vec![p], Category::Convert, true);
    }
}

// --- App -------------------------------------------------------------------------

struct TrayItems {
    open: MenuItem<tauri::Wry>,
    float: MenuItem<tauri::Wry>,
    toggle: CheckMenuItem<tauri::Wry>,
    settings: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
}

fn apply_tray_visibility(app: &AppHandle) {
    let hidden = state(app).settings.lock().unwrap().hide_menu_bar_icon;
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_visible(!hidden);
    }
}

/// Relabel the tray menu after a language change.
fn translate_tray(app: &AppHandle) {
    let l = lang(app);
    if let Some(t) = app.try_state::<TrayItems>() {
        let _ = t.open.set_text(tr(l, OPEN_SELECTION));
        let _ = t.float.set_text(tr(l, "Show Float Shelf"));
        let _ = t.toggle.set_text(tr(l, "Enabled"));
        let _ = t.settings.set_text(tr(l, "Settings…"));
        let _ = t.quit.set_text(tr(l, "Quit Clémentine"));
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let enabled = state(app).settings.lock().unwrap().enabled;
    let l = lang(app);
    let open = MenuItem::with_id(app, "open", tr(l, OPEN_SELECTION), true, None::<&str>)?;
    let toggle =
        CheckMenuItem::with_id(app, "toggle", tr(l, "Enabled"), true, enabled, None::<&str>)?;
    let settings = MenuItem::with_id(
        app,
        "settings",
        tr(l, "Settings…"),
        true,
        Some("CmdOrCtrl+,"),
    )?;
    let float = MenuItem::with_id(app, "float", tr(l, "Show Float Shelf"), true, None::<&str>)?;
    let quit = MenuItem::with_id(
        app,
        "quit",
        tr(l, "Quit Clémentine"),
        true,
        Some("CmdOrCtrl+Q"),
    )?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &float,
            &PredefinedMenuItem::separator(app)?,
            &toggle,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    app.manage(TrayItems {
        open: open.clone(),
        float: float.clone(),
        toggle: toggle.clone(),
        settings: settings.clone(),
        quit: quit.clone(),
    });
    TrayIconBuilder::with_id("main")
        .icon(tauri::image::Image::from_bytes(include_bytes!(
            "../icons/tray.png"
        ))?)
        .icon_as_template(true)
        .tooltip("Clémentine")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => open_for_selection(app),
            "float" => open_float(app, Vec::new()),
            "settings" => open_settings(app),
            "toggle" => {
                let st = state(app);
                let mut s = st.settings.lock().unwrap();
                s.enabled = !s.enabled;
                let _ = s.save(&st.settings_path);
                let _ = toggle.set_checked(s.enabled);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn main() {
    let builder = tauri::Builder::default();
    // A second launch hands its arguments to the running app: files from the
    // right-click menu, or nothing (opened from the Start menu) → Settings.
    #[cfg(windows)]
    let builder =
        builder.plugin(tauri_plugin_single_instance::init(
            |app, args, _cwd| match wheel_args(&args) {
                Some(files) => queue_selection(app, files),
                None => open_settings(app),
            },
        ));
    let app = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            wheel_pick,
            wheel_category,
            wheel_close,
            wheel_hover,
            test_haptic,
            get_settings,
            save_settings,
            ui_language,
            operation_catalog,
            choose_folder,
            engine_status,
            install_engine,
            check_update,
            restart_app,
            set_autostart,
            float_items,
            float_drag,
            float_remove,
            float_open_wheel,
            editor_file,
            editor_apply,
            editor_close,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let first_run = !settings_path.exists();
            let settings = Settings::load(&settings_path);
            let shortcut = settings.shortcut.clone();
            app.manage(AppState {
                settings: Mutex::new(settings),
                settings_path,
                session: Mutex::new(Session::default()),
                floating: Mutex::new(Vec::new()),
                editing: Mutex::new(None),
            });

            let handle = app.handle().clone();
            build_tray(&handle)?;
            apply_tray_visibility(&handle);
            wheel_window(&handle)?;
            if let Err(e) = register_shortcut(&handle, &shortcut) {
                eprintln!("could not register shortcut {shortcut}: {e}");
            }
            let watcher_handle = handle.clone();
            watcher::spawn(move |e| on_watch_event(&watcher_handle, e));
            updater::spawn_auto_check(handle.clone());
            #[cfg(windows)]
            if let Err(e) =
                windows::register_context_menu(tr(lang(&handle), "Open with Clémentine"))
            {
                eprintln!("could not add the right-click menu entry: {e}");
            }
            // `clementine-app --wheel <files…>`: open the wheel for these files
            // (the Windows right-click menu, and handy for development).
            let args: Vec<String> = std::env::args().collect();
            if let Some(files) = wheel_args(&args) {
                queue_selection(&handle, files);
            } else if first_run {
                open_settings(&handle);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Clémentine");

    app.run(|_app, event| match event {
        // Closing the settings window must not quit a menu-bar app.
        RunEvent::ExitRequested {
            api, code: None, ..
        } => api.prevent_exit(),
        // Launching the app again (Finder, Spotlight…) opens Settings: the
        // way back when the menu-bar icon is hidden.
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => open_settings(_app),
        // Finder's "Open With → Clémentine", or `open -a Clémentine <files>`.
        #[cfg(target_os = "macos")]
        RunEvent::Opened { urls } => {
            let paths: Vec<PathBuf> = urls.iter().filter_map(|u| u.to_file_path().ok()).collect();
            if !paths.is_empty() {
                queue_selection(_app, paths);
            }
        }
        _ => {}
    });
}
