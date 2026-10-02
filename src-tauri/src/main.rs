#![cfg_attr(not(test), windows_subsystem = "windows")]
mod core;
mod native_icons;
mod platform;
use core::*;
use std::{
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager,
};
#[derive(serde::Serialize, Clone)]
struct Frame {
    profile: Profile,
    theme: WheelTheme,
    selected: Option<usize>,
    center: (f64, f64),
    native_icons: Vec<Option<String>>,
}
struct Shared {
    config: Mutex<Config>,
    path: std::path::PathBuf,
    usage: Mutex<UsageStore>,
    usage_path: std::path::PathBuf,
    ready: std::sync::atomic::AtomicBool,
}
// Opt-in debug-only test evidence. Never stores window titles, process paths or input text.
fn trace(event: &str) {
    if cfg!(debug_assertions) {
        if let Ok(path) = std::env::var("CWE_TEST_TRACE") {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(f, "{event}");
            }
        }
    }
}
#[tauri::command]
fn get_config(state: tauri::State<Arc<Shared>>) -> Result<String, String> {
    serde_yaml::to_string(&*state.config.lock().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
#[tauri::command]
fn list_running_applications() -> Vec<platform::RunningApplication> {
    platform::running_applications()
}
fn persist_usage(path: &std::path::Path, usage: &UsageStore) -> Result<(), String> {
    let yaml = serde_json::to_vec_pretty(usage).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, yaml).map_err(|e| e.to_string())?;
    if path.exists() {
        std::fs::copy(path, path.with_extension("bak")).map_err(|e| e.to_string())?;
    }
    std::fs::rename(tmp, path).map_err(|e| e.to_string())
}
#[tauri::command]
fn get_adaptive_status(state: tauri::State<Arc<Shared>>) -> Result<AdaptiveStatus, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    let usage = state.usage.lock().map_err(|e| e.to_string())?;
    Ok(usage.status(&config))
}
#[tauri::command]
fn set_adaptive_enabled(enabled: bool, state: tauri::State<Arc<Shared>>) -> Result<(), String> {
    let mut usage = state.usage.lock().map_err(|e| e.to_string())?;
    usage.enabled = enabled;
    persist_usage(&state.usage_path, &usage)
}
#[tauri::command]
fn set_wheel_theme(theme: String, state: tauri::State<Arc<Shared>>) -> Result<(), String> {
    let mut usage = state.usage.lock().map_err(|e| e.to_string())?;
    usage.theme = WheelTheme::parse(&theme)?;
    persist_usage(&state.usage_path, &usage)
}
#[tauri::command]
fn set_active_mode(
    application: String,
    mode: String,
    state: tauri::State<Arc<Shared>>,
) -> Result<(), String> {
    let application_key = application.to_lowercase();
    if !mode.is_empty() {
        let config = state.config.lock().map_err(|e| e.to_string())?;
        let exists = config.profiles.iter().any(|profile| {
            profile.scope == Scope::Mode
                && profile
                    .application
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(&application))
                && profile.mode.as_deref() == Some(mode.as_str())
        });
        if !exists {
            return Err("请先保存此软件的场景轮盘，再设为当前场景".into());
        }
    }
    let mut usage = state.usage.lock().map_err(|e| e.to_string())?;
    if mode.is_empty() {
        usage.active_modes.remove(&application_key);
    } else {
        usage.active_modes.insert(application_key, mode);
    }
    persist_usage(&state.usage_path, &usage)
}
#[tauri::command]
fn reset_adaptive(state: tauri::State<Arc<Shared>>) -> Result<(), String> {
    let mut usage = state.usage.lock().map_err(|e| e.to_string())?;
    usage.reset();
    persist_usage(&state.usage_path, &usage)
}
#[tauri::command]
fn export_habits(path: String, state: tauri::State<Arc<Shared>>) -> Result<(), String> {
    let destination = std::path::PathBuf::from(path);
    let usage = state.usage.lock().map_err(|e| e.to_string())?.clone();
    write_portable_habits(&destination, &usage)
}
#[tauri::command]
fn preview_habits_import(
    json: String,
    state: tauri::State<Arc<Shared>>,
) -> Result<HabitsImportReport, String> {
    let incoming = PortableHabitsFile::parse(&json)?;
    let config = state.config.lock().map_err(|e| e.to_string())?;
    let mut preview = state.usage.lock().map_err(|e| e.to_string())?.clone();
    Ok(preview.merge_portable(&incoming.data, &config))
}
#[tauri::command]
fn import_habits(
    json: String,
    state: tauri::State<Arc<Shared>>,
) -> Result<HabitsImportReport, String> {
    let incoming = PortableHabitsFile::parse(&json)?;
    let config = state.config.lock().map_err(|e| e.to_string())?;
    let mut usage = state.usage.lock().map_err(|e| e.to_string())?;
    let mut merged = usage.clone();
    let report = merged.merge_portable(&incoming.data, &config);
    persist_usage(&state.usage_path, &merged)?;
    *usage = merged;
    Ok(report)
}
#[tauri::command]
fn renderer_ready(window: tauri::WebviewWindow, state: tauri::State<Arc<Shared>>) -> bool {
    if window.label() == "overlay" {
        state
            .ready
            .store(true, std::sync::atomic::Ordering::Relaxed);
        trace("ready");
    }
    cfg!(debug_assertions) && std::env::var_os("CWE_TEST_TRACE").is_some()
}
#[tauri::command]
fn render_probe(
    window: tauri::WebviewWindow,
    profile: String,
    sectors: usize,
    selected: String,
    icons: usize,
    caption: String,
    selected_icon: String,
    fitted: bool,
    native_icons: usize,
    theme: String,
    sector_labels: usize,
    labels_fitted: bool,
    center_logo: bool,
    native_icons_loaded: usize,
    native_icon_fallbacks: usize,
    native_icon_failures: usize,
) {
    if window.label() == "overlay" && cfg!(debug_assertions) {
        trace(&format!(
            "render:{profile}:{sectors}:{selected}:{icons}:{caption}:{selected_icon}:{fitted}:native_icons={native_icons}:theme={theme}:labels={sector_labels}:labels_fitted={labels_fitted}:center_logo={center_logo}:native_loaded={native_icons_loaded}:native_fallbacks={native_icon_fallbacks}:native_failures={native_icon_failures}"
        ));
    }
}
#[tauri::command]
fn persist_config(c: Config, yaml: &str, state: &Shared) -> Result<(), String> {
    c.validate()?;
    let mut lock = state.config.lock().map_err(|e| e.to_string())?;
    let changed: Vec<String> = lock
        .profiles
        .iter()
        .filter(|old| matches!(old.scope, Scope::Application | Scope::Mode))
        .filter(|old| {
            c.profiles
                .iter()
                .find(|new| new.id == old.id)
                .is_none_or(|new| serde_json::to_vec(*old).ok() != serde_json::to_vec(new).ok())
        })
        .map(|profile| profile.id.clone())
        .collect();
    if !changed.is_empty() {
        let mut usage = state.usage.lock().map_err(|e| e.to_string())?;
        for id in changed {
            usage.forget_profile(&id);
        }
        usage.active_modes.retain(|application, mode| {
            c.profiles.iter().any(|profile| {
                profile.scope == Scope::Mode
                    && profile
                        .application
                        .as_ref()
                        .is_some_and(|name| name.eq_ignore_ascii_case(application))
                    && profile.mode.as_ref().is_some_and(|key| key == mode)
            })
        });
        persist_usage(&state.usage_path, &usage)?;
    }
    let tmp = state.path.with_extension("tmp");
    std::fs::write(&tmp, yaml).map_err(|e| e.to_string())?;
    if state.path.exists() {
        std::fs::copy(&state.path, state.path.with_extension("bak")).map_err(|e| e.to_string())?;
    }
    std::fs::rename(tmp, &state.path).map_err(|e| e.to_string())?;
    platform::update_trigger(c.trigger);
    *lock = c;
    Ok(())
}
#[tauri::command]
fn save_config(yaml: String, state: tauri::State<Arc<Shared>>) -> Result<(), String> {
    let c: Config = serde_yaml::from_str(&yaml).map_err(|e| e.to_string())?;
    persist_config(c, &yaml, &state)
}
#[tauri::command]
fn set_profile_order(
    profile_id: String,
    order: Vec<String>,
    state: tauri::State<Arc<Shared>>,
) -> Result<(), String> {
    if state.usage.lock().map_err(|e| e.to_string())?.enabled {
        return Err("请先选择“固定位置，保留方向记忆”再手动调整".into());
    }
    let mut config = state.config.lock().map_err(|e| e.to_string())?.clone();
    let profile = config
        .profiles
        .iter_mut()
        .find(|profile| profile.id == profile_id)
        .ok_or("找不到该软件 Profile")?;
    *profile = reorder_profile(profile, &order)?;
    config.validate()?;
    let yaml = serde_yaml::to_string(&config).map_err(|e| e.to_string())?;
    persist_config(config, &yaml, &state)
}
fn main() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_config,
            list_running_applications,
            get_adaptive_status,
            set_adaptive_enabled,
            set_wheel_theme,
            set_active_mode,
            set_profile_order,
            reset_adaptive,
            export_habits,
            preview_habits_import,
            import_habits,
            save_config,
            renderer_ready,
            render_probe
        ])
        .setup(|app| {
            trace("startup:setup");
            platform::single_instance().map_err(std::io::Error::other)?;
            let config_dir = if cfg!(debug_assertions) {
                std::env::var_os("CWE_TEST_CONFIG_DIR")
                    .map(std::path::PathBuf::from)
                    .unwrap_or(app.path().app_config_dir()?)
            } else {
                app.path().app_config_dir()?
            };
            let path = config_dir.join("profiles.yaml");
            std::fs::create_dir_all(path.parent().unwrap())?;
            let first_run = !path.exists();
            if first_run {
                std::fs::write(&path, include_str!("../profiles/default.yaml"))?;
            }
            let mut config: Config = serde_yaml::from_str(&std::fs::read_to_string(&path)?)?;
            let icon_migration = config_dir.join("migration-v0.1.3-autocad-icons.done");
            if !icon_migration.exists() {
                let defaults: Config =
                    serde_yaml::from_str(include_str!("../profiles/default.yaml"))?;
                let filled = config.fill_missing_default_autocad_icons(&defaults);
                if filled > 0 {
                    config.validate().map_err(std::io::Error::other)?;
                    let backup = config_dir.join("profiles.pre-v0.1.3-autocad-icons.yaml");
                    std::fs::copy(&path, backup)?;
                    let tmp = config_dir.join("profiles.v0.1.3.tmp");
                    std::fs::write(&tmp, serde_yaml::to_string(&config)?)?;
                    std::fs::rename(tmp, &path)?;
                }
                std::fs::write(icon_migration, format!("filled={filled}\n"))?;
            }
            config.expand_legacy_autocad_outer_rings();
            config.validate().map_err(std::io::Error::other)?;
            let usage_path = path.with_file_name("usage.json");
            let usage = if usage_path.exists() {
                std::fs::read(&usage_path)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<UsageStore>(&bytes).ok())
                    .filter(|store| store.validate().is_ok())
                    .unwrap_or_default()
            } else {
                UsageStore::default()
            };
            let shared = Arc::new(Shared {
                config: Mutex::new(config),
                path,
                usage: Mutex::new(usage),
                usage_path,
                ready: std::sync::atomic::AtomicBool::new(false),
            });
            app.manage(shared.clone());
            let _native_icon_warmup = std::thread::Builder::new()
                .name("context-wheel-native-icons".into())
                .spawn(native_icons::warm);
            // Create WebViews only after IPC state exists. Default window creation
            // can finish navigation before setup and race the first invoke.
            let overlay =
                tauri::WebviewWindowBuilder::from_config(app, &app.config().app.windows[0])?
                    .build()?;
            let studio_window =
                tauri::WebviewWindowBuilder::from_config(app, &app.config().app.windows[1])?
                    .build()?;
            overlay.set_ignore_cursor_events(true)?;
            overlay.set_focusable(false)?;
            let studio = MenuItem::with_id(app, "studio", "Profile Studio", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&studio, &quit])?;
            // SAYELF mark, pre-rendered by tools/make-brand-assets.py as raw 32x32 RGBA.
            let icon = tauri::image::Image::new_owned(
                include_bytes!("../icons/tray-32.rgba").to_vec(),
                32,
                32,
            );
            TrayIconBuilder::new()
                .icon(icon)
                .tooltip("Context Wheel · 鼠标侧键 4")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "studio" => {
                        if let Some(w) = app.get_webview_window("studio") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            let (tx, rx) = mpsc::channel();
            if cfg!(debug_assertions) && std::env::var_os("CWE_TEST_SKIP_HOOK").is_some() {
                trace("startup:hook_skipped_for_test");
            } else {
                platform::set_trigger(shared.config.lock().unwrap().trigger);
                platform::install(tx).map_err(std::io::Error::other)?;
                trace("startup:hook_installed");
            }
            if first_run {
                studio_window.show()?;
                studio_window.set_focus()?;
                trace("startup:first_run_studio");
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut gesture = Gesture::default();
                let mut target: Option<Context> = None;
                let mut profile: Option<Profile> = None;
                let mut current_native_icons = Vec::new();
                let mut deadline: Option<Instant> = None;
                let mut center = (180., 180.);
                let mut theme = WheelTheme::default();
                let mut dead = 54.;
                let mut outer = 162.;
                let mut cancelled = false;
                loop {
                    let result = if let Some(d) = deadline {
                        rx.recv_timeout(d.saturating_duration_since(Instant::now()))
                    } else {
                        rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
                    };
                    match result {
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            deadline = None;
                            if target.as_ref().is_some_and(platform::same_target) {
                                gesture.show();
                                if let Some(p) = &profile {
                                    let _ = overlay.emit(
                                        "wheel",
                                        Frame {
                                            profile: p.clone(),
                                            theme,
                                            selected: gesture.selected,
                                            center,
                                            native_icons: current_native_icons.clone(),
                                        },
                                    );
                                    let _ = overlay.show();
                                    trace("visible");
                                }
                            } else {
                                cancelled = true;
                                let _ = overlay.hide();
                                trace("cancel:target_changed");
                            }
                        }
                        Ok(platform::InputEvent::Down(x, y)) => {
                            let _ = overlay.hide();
                            gesture.reset();
                            cancelled = false;
                            deadline = None;
                            target = None;
                            profile = None;
                            current_native_icons.clear();
                            if !shared.ready.load(std::sync::atomic::Ordering::Relaxed) {
                                cancelled = true;
                                continue;
                            }
                            match platform::foreground() {
                                Ok(mut c) => {
                                    c.mode = shared
                                        .usage
                                        .lock()
                                        .unwrap()
                                        .active_modes
                                        .get(&c.process.to_lowercase())
                                        .cloned();
                                    let base = {
                                        let cfg = shared.config.lock().unwrap();
                                        cfg.resolve(&c).clone()
                                    };
                                    let (p, active_theme) = {
                                        let mut usage = shared.usage.lock().unwrap();
                                        let prior = usage.layouts.get(&base.id).cloned();
                                        let adapted = usage.adapt_profile(&base);
                                        if usage.layouts.get(&base.id).cloned() != prior {
                                            if let Err(e) =
                                                persist_usage(&shared.usage_path, &usage)
                                            {
                                                let _ = handle.emit(
                                                    "status",
                                                    format!("本地习惯布局保存失败：{e}"),
                                                );
                                            }
                                        }
                                        (adapted, usage.theme)
                                    };
                                    theme = active_theme;
                                    current_native_icons =
                                        native_icons::for_profile(&c.process, &c.process_path, &p);
                                    let ring_count = p.ring_count();
                                    let (l, t, r, b) = platform::monitor(x, y);
                                    let _ =
                                        overlay.set_position(tauri::PhysicalPosition::new(x, y));
                                    let scale = overlay.scale_factor().unwrap_or(1.);
                                    let size = 360. * scale;
                                    dead = wheel_dead_zone(ring_count) * scale;
                                    outer = 162. * scale;
                                    let (px, py) = place(x as f64, y as f64, l, t, r, b, size);
                                    center = (180., 180.);
                                    let _ = overlay.set_position(tauri::PhysicalPosition::new(
                                        px as i32, py as i32,
                                    ));
                                    let _ = overlay.set_size(tauri::PhysicalSize::new(
                                        size as u32,
                                        size as u32,
                                    ));
                                    gesture.down(x as f64, y as f64);
                                    let _ = overlay.emit(
                                        "wheel",
                                        Frame {
                                            profile: p.clone(),
                                            theme,
                                            selected: None,
                                            center,
                                            native_icons: current_native_icons.clone(),
                                        },
                                    );
                                    trace(&format!("down:{}", p.id));
                                    target = Some(c);
                                    profile = Some(p);
                                    deadline = Some(Instant::now() + Duration::from_millis(80));
                                }
                                Err(e) => {
                                    cancelled = true;
                                    let _ = handle.emit("status", e);
                                }
                            }
                        }
                        Ok(platform::InputEvent::Move(x, y)) => {
                            if !cancelled && target.is_some() {
                                gesture.moved(
                                    x as f64,
                                    y as f64,
                                    dead,
                                    profile.as_ref().expect("active gesture has a profile"),
                                    outer,
                                );
                                if gesture.visible {
                                    let _ = overlay.emit("selection", gesture.selected);
                                }
                                trace(&format!("selection:{:?}", gesture.selected));
                            }
                        }
                        Ok(platform::InputEvent::Cancel) => {
                            cancelled = true;
                            deadline = None;
                            gesture.state = WheelState::Cancelled;
                            let _ = overlay.hide();
                        }
                        Ok(platform::InputEvent::Up(x, y)) => {
                            deadline = None;
                            let _ = overlay.hide();
                            if !cancelled && target.is_some() {
                                gesture.moved(
                                    x as f64,
                                    y as f64,
                                    dead,
                                    profile.as_ref().expect("active gesture has a profile"),
                                    outer,
                                );
                                if let Some(i) = gesture.release() {
                                    let p = profile.as_ref().unwrap();
                                    if let Some(s) = p.sector_at(i).filter(|s| s.enabled) {
                                        let outcome =
                                            platform::execute(&s.action, target.as_ref().unwrap());
                                        if outcome.is_ok()
                                            && matches!(p.scope, Scope::Application | Scope::Mode)
                                        {
                                            let mut usage = shared.usage.lock().unwrap();
                                            if usage.enabled {
                                                usage.record_success(p, s);
                                                let _ = usage.adapt_profile(p);
                                                if let Err(e) =
                                                    persist_usage(&shared.usage_path, &usage)
                                                {
                                                    let _ = handle.emit(
                                                        "status",
                                                        format!("本地使用次数保存失败：{e}"),
                                                    );
                                                }
                                            }
                                        }
                                        trace(if outcome.is_ok() {
                                            "execute:ok"
                                        } else {
                                            "execute:rejected"
                                        });
                                        let result = outcome.unwrap_or_else(|e| e);
                                        let _ = handle.emit("status", result);
                                    } else {
                                        trace("execute:empty_slot");
                                        let _ = handle.emit("status", "该位置尚未配置命令");
                                    }
                                } else {
                                    trace("cancel:dead_zone");
                                }
                            }
                            gesture.reset();
                            target = None;
                            profile = None;
                            cancelled = false;
                            platform::set_trigger(shared.config.lock().unwrap().trigger);
                        }
                    }
                }
            });
            // Studio close hides the configuration window; the engine remains in the tray.
            let studio = app.get_webview_window("studio").unwrap();
            let copy = studio.clone();
            studio.on_window_event(move |e| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = e {
                    api.prevent_close();
                    let _ = copy.hide();
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!());
    if let Err(e) = result {
        trace(&format!("startup_error:{e}"));
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
            let message: Vec<u16> = format!("Context Wheel 启动失败：{e}")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let title: Vec<u16> = "Context Wheel".encode_utf16().chain(Some(0)).collect();
            MessageBoxW(
                std::ptr::null_mut(),
                message.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
        std::process::exit(1)
    }
}
