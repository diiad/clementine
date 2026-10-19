//! Clémentine: a menu-bar app. Drag a file, hold a key, and a wheel of
//! conversions, tools or actions opens under the cursor; drop on a slice.

mod native;
mod settings;
mod watcher;
mod wheel;

use clementine_core::{operations, Category, Operation};
use serde::Serialize;
use settings::Settings;
use std::path::PathBuf;
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
}

#[derive(Clone, Serialize)]
struct ShowPayload {
    category: Category,
    title: String,
    segments: Vec<Segment>,
    click_mode: bool,
    hub: f64,
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

fn title_for(paths: &[PathBuf]) -> String {
    match paths {
        [one] => one
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        many => format!("{} items", many.len()),
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
        .accept_first_mouse(true)
        .build()?;
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
    let ops = operations(&paths, category);
    let segments = wheel::layout(&ops);
    let payload = ShowPayload {
        category,
        title: title_for(&paths),
        segments: segments.clone(),
        click_mode,
        hub: wheel::HUB,
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
        if let Ok(cursor) = app.cursor_position() {
            let scale = app
                .monitor_from_point(cursor.x, cursor.y)
                .ok()
                .flatten()
                .map(|m| m.scale_factor())
                .unwrap_or(2.0);
            let half = wheel::SIZE * scale / 2.0;
            let _ = window.set_position(PhysicalPosition::new(cursor.x - half, cursor.y - half));
        }
    }
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

fn on_wheel_event(app: &AppHandle, event: &WindowEvent) {
    let WindowEvent::DragDrop(dd) = event else {
        return;
    };
    let Some(window) = app.get_webview_window("wheel") else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(2.0);
    let locate = |p: &PhysicalPosition<f64>| {
        // On macOS wry reports drag positions in points even though Tauri
        // types them as physical pixels.
        let l = if cfg!(target_os = "macos") {
            tauri::LogicalPosition::new(p.x, p.y)
        } else {
            p.to_logical::<f64>(scale)
        };
        let st = state(app);
        let s = st.session.lock().unwrap();
        wheel::hit(&s.segments, l.x, l.y)
    };
    match dd {
        DragDropEvent::Enter { position, .. } | DragDropEvent::Over { position } => {
            let index = match locate(position) {
                Hit::Segment(i) => i as i64,
                Hit::Hub => -2,
                Hit::Outside => -1,
            };
            let _ = window.emit("wheel:hover", index);
        }
        DragDropEvent::Leave => {
            let _ = window.emit("wheel:hover", -1);
        }
        DragDropEvent::Drop { paths, position } => match locate(position) {
            Hit::Segment(i) => {
                let paths = if paths.is_empty() {
                    state(app).session.lock().unwrap().paths.clone()
                } else {
                    paths.clone()
                };
                start_job(app, i, paths);
            }
            _ => hide_wheel(app),
        },
        _ => {}
    }
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

/// Run the operation in slice `index` on `paths`. Called on the main thread.
fn start_job(app: &AppHandle, index: usize, paths: Vec<PathBuf>) {
    let (op, opts, reveal) = {
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
        (op, settings.options(), settings.reveal_after)
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
        "action:move" => {
            hide_wheel(app);
            let app = app.clone();
            std::thread::spawn(move || {
                let Some(dir) = app
                    .dialog()
                    .file()
                    .set_title("Move to…")
                    .blocking_pick_folder()
                else {
                    return;
                };
                let Ok(dir) = dir.into_path() else { return };
                if let Err(e) = native::move_into(&paths, &dir) {
                    notify(&app, "Move failed", &e.to_string());
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
        let result = clementine_core::run(&op.id, &paths, &opts, &progress);
        let quick = op.id.starts_with("action:");
        let linger = match &result {
            Ok(outputs) => {
                let message = match outputs.as_slice() {
                    [] => "Done".to_string(),
                    [one] => one
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    many => format!("{} files", many.len()),
                };
                emit_status(
                    &app,
                    Status::Done {
                        message: message.clone(),
                    },
                );
                if started.elapsed() > Duration::from_secs(5) {
                    notify(&app, &format!("{} finished", op.label), &message);
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
                notify(&app, &format!("{} failed", op.label), &e.to_string());
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
                let (visible, current, busy) = {
                    let st = state(&handle);
                    let s = st.session.lock().unwrap();
                    (s.visible, s.category, s.busy)
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
                notify(&h, "Clémentine", "Select files in Finder first.");
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
            let Ok(w) =
                WebviewWindowBuilder::new(app, "float", WebviewUrl::App("float.html".into()))
                    .title("Float")
                    .inner_size(260.0, 320.0)
                    .min_inner_size(200.0, 120.0)
                    .always_on_top(true)
                    .visible_on_all_workspaces(true)
                    .skip_taskbar(true)
                    .title_bar_style(tauri::TitleBarStyle::Overlay)
                    .hidden_title(true)
                    .build()
            else {
                return;
            };
            w
        }
    };
    let _ = window.emit("float:update", ());
    let _ = window.show();
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
            .title("Clémentine Settings")
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

#[tauri::command]
fn wheel_close(app: AppHandle) {
    if !state(&app).session.lock().unwrap().busy {
        hide_wheel(&app);
    }
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
            return Err(format!("This shortcut can't be used: {e}"));
        }
    }
    settings
        .save(&st.settings_path)
        .map_err(|e| e.to_string())?;
    *st.settings.lock().unwrap() = settings.clone();
    if let Some(item) = app.try_state::<CheckMenuItem<tauri::Wry>>() {
        let _ = item.set_checked(settings.enabled);
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
    version: &'static str,
    ffmpeg: Option<String>,
    libreoffice: Option<String>,
    autostart: bool,
}

#[tauri::command]
fn engine_status(app: AppHandle) -> Engines {
    Engines {
        version: env!("CARGO_PKG_VERSION"),
        ffmpeg: clementine_core::engines::find_tool("ffmpeg").map(|p| p.display().to_string()),
        libreoffice: clementine_core::engines::document::office().map(|p| p.display().to_string()),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
    }
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

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let enabled = state(app).settings.lock().unwrap().enabled;
    let shortcut_label = "Open Wheel for Finder Selection";
    let open = MenuItem::with_id(app, "open", shortcut_label, true, None::<&str>)?;
    let toggle = CheckMenuItem::with_id(app, "toggle", "Enabled", true, enabled, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?;
    let float = MenuItem::with_id(app, "float", "Show Float Shelf", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Clémentine", true, Some("CmdOrCtrl+Q"))?;
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
    app.manage(toggle.clone());
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
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
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
            get_settings,
            save_settings,
            choose_folder,
            engine_status,
            set_autostart,
            float_items,
            float_drag,
            float_remove,
            float_open_wheel,
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
            });

            let handle = app.handle().clone();
            build_tray(&handle)?;
            wheel_window(&handle)?;
            if let Err(e) = register_shortcut(&handle, &shortcut) {
                eprintln!("could not register shortcut {shortcut}: {e}");
            }
            let watcher_handle = handle.clone();
            watcher::spawn(move |e| on_watch_event(&watcher_handle, e));
            // `clementine-app --wheel <files…>`: open the wheel right away (handy for development).
            let args: Vec<String> = std::env::args().collect();
            if let Some(i) = args.iter().position(|a| a == "--wheel") {
                let files: Vec<PathBuf> = args[i + 1..]
                    .iter()
                    .map(PathBuf::from)
                    .filter(|p| p.exists())
                    .collect();
                if !files.is_empty() {
                    let h = handle.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(600));
                        let h2 = h.clone();
                        let _ = h.run_on_main_thread(move || {
                            open_wheel(&h2, files, Category::Convert, true)
                        });
                    });
                }
            } else if first_run {
                open_settings(&handle);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Clémentine");

    app.run(|_, event| {
        // Closing the settings window must not quit a menu-bar app.
        if let RunEvent::ExitRequested {
            api, code: None, ..
        } = event
        {
            api.prevent_exit();
        }
    });
}
