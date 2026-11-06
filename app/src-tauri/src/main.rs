// 专注悬浮时钟入口。核心计时/存储逻辑见 timer.rs / storage.rs。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod storage;
mod timer;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Local;
use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_notification::NotificationExt;

use storage::{Session, Store};
use timer::{Phase, Timer, TimerError, DEFAULT_DURATION_SECS};

struct AppState {
    timer: Mutex<Timer>,
    store: Mutex<Store>,
    start_at: Mutex<Option<chrono::DateTime<Local>>>,
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    last_size_save: Mutex<Instant>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TimerStateDto {
    phase: String,
    remaining_secs: u64,
    total_secs: u64,
    task: Option<String>,
    today_total_secs: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StatsDto {
    dates: Vec<String>,
    sessions: Vec<Session>,
    selected_date: String,
    selected_total_secs: u64,
    today_total_secs: u64,
    today_date: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
struct Settings {
    autostart: bool,
    window_width: Option<u32>,
    window_height: Option<u32>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            autostart: true,
            window_width: None,
            window_height: None,
        }
    }
}

fn today_date() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn make_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{nanos:x}")
}

fn phase_str(p: Phase) -> String {
    match p {
        Phase::Idle => "idle".to_string(),
        Phase::Running => "running".to_string(),
        Phase::Paused => "paused".to_string(),
        Phase::Finished => "finished".to_string(),
    }
}

fn err_str(e: TimerError) -> String {
    match e {
        TimerError::EmptyTask => "任务不能为空".to_string(),
        TimerError::InvalidDuration => "时长必须大于 0".to_string(),
        TimerError::InvalidState => "当前状态不允许该操作".to_string(),
    }
}

fn current_state_from(state: &Arc<AppState>) -> TimerStateDto {
    let timer = state.timer.lock().unwrap();
    let store = state.store.lock().unwrap();
    let today = today_date();
    TimerStateDto {
        phase: phase_str(timer.phase()),
        remaining_secs: timer.remaining_secs(),
        total_secs: timer.total_secs(),
        task: timer.task().map(|s| s.to_string()),
        today_total_secs: store.total_seconds_on(&today),
    }
}

/// 完成当前会话并落盘。`natural=true` 表示自然归零，`false` 表示手动提前结束。
/// 返回落盘的会话记录。
fn record_finish(state: &Arc<AppState>, natural: bool) -> Result<Session, String> {
    let mut timer = state.timer.lock().unwrap();
    if !natural {
        timer.finish().map_err(err_str)?;
    }
    let completed = timer
        .take_completed_session()
        .ok_or_else(|| "没有可记录的会话".to_string())?;
    drop(timer);

    let now = Local::now();
    let start_at = state.start_at.lock().unwrap().take().unwrap_or(now);
    let session = Session {
        id: make_id(),
        date: start_at.format("%Y-%m-%d").to_string(),
        start_at: start_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
        end_at: now.format("%Y-%m-%dT%H:%M:%S").to_string(),
        duration_secs: completed.duration_secs,
        task: completed.task,
    };

    let mut store = state.store.lock().unwrap();
    store.add(session.clone()).map_err(|e| format!("保存失败：{e:?}"))?;
    Ok(session)
}

fn notify_finish(app: &AppHandle, task: &str, duration_secs: u64) {
    let minutes = duration_secs / 60;
    let body = if minutes > 0 {
        format!("「{task}」专注 {minutes} 分钟，已记录")
    } else {
        format!("「{task}」专注完成，已记录")
    };
    let _ = app
        .notification()
        .builder()
        .title("专注完成")
        .body(body)
        .show();
}

/// 3x5 点阵字体的字形（0-9 与冒号）。
fn glyph(c: char) -> [u8; 5] {
    match c {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b001, 0b001, 0b001],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        ':' => [0b000, 0b010, 0b000, 0b010, 0b000],
        _ => [0b000, 0b000, 0b000, 0b000, 0b000],
    }
}

/// 把倒计时文字渲染成方形托盘图标（白字深底），数字撑满方形，保证在托盘里可读。
fn render_countdown_icon(text: &str) -> tauri::image::Image<'static> {
    const SCALE: usize = 6;
    const GW: usize = 3;
    const GH: usize = 5;
    const GAP: usize = 1;

    let chars: Vec<char> = text.chars().collect();
    let n = chars.len().max(1);
    let cw = n * GW * SCALE + (n - 1) * GAP * SCALE;
    let ch = GH * SCALE;
    let side = cw.max(ch) + 2 * SCALE;
    let w = side;
    let h = side;
    let ox = (w - cw) / 2;
    let oy = (h - ch) / 2;

    let mut rgba = vec![0u8; w * h * 4];
    for px in rgba.chunks_mut(4) {
        px[0] = 26;
        px[1] = 28;
        px[2] = 36;
        px[3] = 255;
    }

    let mut x = ox;
    for &c in &chars {
        let g = glyph(c);
        for row in 0..GH {
            for col in 0..GW {
                if (g[row] >> (GW - 1 - col)) & 1 == 1 {
                    for dy in 0..SCALE {
                        for dx in 0..SCALE {
                            let xi = x + col * SCALE + dx;
                            let yi = oy + row * SCALE + dy;
                            let idx = (yi * w + xi) * 4;
                            rgba[idx] = 255;
                            rgba[idx + 1] = 255;
                            rgba[idx + 2] = 255;
                            rgba[idx + 3] = 255;
                        }
                    }
                }
            }
        }
        x += GW * SCALE + GAP * SCALE;
    }

    tauri::image::Image::new_owned(rgba, w as u32, h as u32)
}

fn update_tray(app: &AppHandle, state: &TimerStateDto) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        if state.phase == "running" || state.phase == "paused" {
            // 托盘只显示两位数字：≥60 秒显示分钟，最后 1 分钟显示秒。
            let text = if state.remaining_secs >= 60 {
                format!("{:02}", state.remaining_secs / 60)
            } else {
                format!("{:02}", state.remaining_secs)
            };
            let tip = format!(
                "{:02}:{:02}",
                state.remaining_secs / 60,
                state.remaining_secs % 60
            );
            let _ = tray.set_icon(Some(render_countdown_icon(&text)));
            let _ = tray.set_tooltip(Some(tip));
        } else {
            if let Some(icon) = app.default_window_icon().cloned() {
                let _ = tray.set_icon(Some(icon));
            }
            let _ = tray.set_tooltip(Some("专注时钟".to_string()));
        }
    }
}

fn start_tick_thread(app: AppHandle, state: Arc<AppState>) {
    std::thread::spawn(move || {
        let mut last = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(250));
            let now = Instant::now();
            let whole = (now - last).as_secs();
            if whole == 0 {
                continue;
            }
            last = now;

            let mut natural = false;
            {
                let mut timer = state.timer.lock().unwrap();
                if timer.phase() == Phase::Running {
                    if let Ok(true) = timer.elapse(whole) {
                        natural = true;
                    }
                }
            }
            if natural {
                if let Ok(session) = record_finish(&state, true) {
                    notify_finish(&app, &session.task, session.duration_secs);
                }
            }
            let dto = current_state_from(&state);
            update_tray(&app, &dto);
            let _ = app.emit("timer-tick", dto);
        }
    });
}

fn data_dir(app: &AppHandle) -> PathBuf {
    // 开发/沙箱下用环境变量重定向到工作区，生产环境默认走 AppData。
    if let Ok(dir) = std::env::var("FOCUS_TIMER_DATA_DIR") {
        return PathBuf::from(dir);
    }
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join("data"))
}

fn load_settings(path: &PathBuf) -> Settings {
    if let Ok(text) = std::fs::read_to_string(path) {
        if let Ok(s) = serde_json::from_str::<Settings>(&text) {
            return s;
        }
    }
    Settings::default()
}

fn save_settings(path: &PathBuf, settings: &Settings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// macOS 风格毛玻璃：Windows 上优先 Acrylic（系统级桌面模糊），失败回退 Blur。
fn apply_window_effects(app: &AppHandle) {
    #[cfg(target_os = "windows")]
    {
        if let Some(window) = app.get_webview_window("main") {
            // Win11 用 Mica（顺滑、原生观感），失败回退 Blur（Win10）。
            if window_vibrancy::apply_mica(&window, Some(true)).is_err() {
                let _ = window_vibrancy::apply_blur(&window, Some((32, 32, 36, 160)));
            }
        }
    }
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn toggle_from_tray(app: &AppHandle) {
    let state = app.state::<Arc<AppState>>();
    {
        let mut timer = state.timer.lock().unwrap();
        match timer.phase() {
            Phase::Running => {
                let _ = timer.pause();
            }
            Phase::Paused => {
                let _ = timer.resume();
            }
            _ => {
                drop(timer);
                show_main_window(app);
                return;
            }
        }
    }
    let _ = app.emit("timer-tick", current_state_from(state.inner()));
}

fn finish_from_tray(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<Arc<AppState>>();
    record_finish(state.inner(), false)?;
    let _ = app.emit("timer-tick", current_state_from(state.inner()));
    Ok(())
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "暂停 / 继续", true, None::<&str>)?;
    let finish = MenuItem::with_id(app, "finish", "结束计时", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &toggle, &finish, &quit])?;

    let _tray = TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("专注时钟")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "toggle" => toggle_from_tray(app),
            "finish" => {
                let _ = finish_from_tray(app);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

#[tauri::command]
fn get_state(state: tauri::State<Arc<AppState>>) -> TimerStateDto {
    current_state_from(state.inner())
}

#[tauri::command]
fn start_timer(
    state: tauri::State<Arc<AppState>>,
    task: String,
    duration_secs: Option<u64>,
) -> Result<TimerStateDto, String> {
    let duration = duration_secs.unwrap_or(DEFAULT_DURATION_SECS);
    {
        let mut timer = state.timer.lock().unwrap();
        timer.start(task, duration).map_err(err_str)?;
    }
    *state.start_at.lock().unwrap() = Some(Local::now());
    Ok(current_state_from(state.inner()))
}

#[tauri::command]
fn pause_timer(state: tauri::State<Arc<AppState>>) -> Result<TimerStateDto, String> {
    state.timer.lock().unwrap().pause().map_err(err_str)?;
    Ok(current_state_from(state.inner()))
}

#[tauri::command]
fn resume_timer(state: tauri::State<Arc<AppState>>) -> Result<TimerStateDto, String> {
    state.timer.lock().unwrap().resume().map_err(err_str)?;
    Ok(current_state_from(state.inner()))
}

#[tauri::command]
fn reset_timer(state: tauri::State<Arc<AppState>>) -> TimerStateDto {
    state.timer.lock().unwrap().reset();
    current_state_from(state.inner())
}

#[tauri::command]
fn finish_timer(state: tauri::State<Arc<AppState>>) -> Result<TimerStateDto, String> {
    record_finish(state.inner(), false)?;
    Ok(current_state_from(state.inner()))
}

#[tauri::command]
fn get_stats(state: tauri::State<Arc<AppState>>, date: Option<String>) -> StatsDto {
    let store = state.store.lock().unwrap();
    let today = today_date();
    let selected = date.unwrap_or_else(|| today.clone());
    StatsDto {
        dates: store.all_dates(),
        sessions: store.sessions_on(&selected).into_iter().cloned().collect(),
        selected_date: selected.clone(),
        selected_total_secs: store.total_seconds_on(&selected),
        today_total_secs: store.total_seconds_on(&today),
        today_date: today,
    }
}

#[tauri::command]
fn minimize_to_tray(app: AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn start_drag(app: AppHandle) -> Result<(), String> {
    let window = app.get_window("main").ok_or("窗口不存在")?;
    window.start_dragging().map_err(|e| e.to_string())
}

#[tauri::command]
fn start_resize_dragging(app: AppHandle, direction: String) -> Result<(), String> {
    let dir = match direction.as_str() {
        "East" => tauri_runtime::ResizeDirection::East,
        "South" => tauri_runtime::ResizeDirection::South,
        "West" => tauri_runtime::ResizeDirection::West,
        "North" => tauri_runtime::ResizeDirection::North,
        "NorthEast" => tauri_runtime::ResizeDirection::NorthEast,
        "NorthWest" => tauri_runtime::ResizeDirection::NorthWest,
        "SouthEast" => tauri_runtime::ResizeDirection::SouthEast,
        "SouthWest" => tauri_runtime::ResizeDirection::SouthWest,
        _ => return Err("未知方向".to_string()),
    };
    let window = app.get_window("main").ok_or("窗口不存在")?;
    window.start_resize_dragging(dir).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_settings(state: tauri::State<Arc<AppState>>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn set_autostart(
    app: AppHandle,
    state: tauri::State<Arc<AppState>>,
    enabled: bool,
) -> Result<Settings, String> {
    {
        let mut settings = state.settings.lock().unwrap();
        settings.autostart = enabled;
        save_settings(&state.settings_path, &settings)?;
    }
    if enabled {
        let _ = app.autolaunch().enable();
    } else {
        let _ = app.autolaunch().disable();
    }
    Ok(state.settings.lock().unwrap().clone())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .setup(|app| {
            let dir = data_dir(app.handle());
            let store_path = dir.join("sessions.json");
            let store = Store::load(store_path.clone()).unwrap_or_else(|_| Store::new(store_path));
            let settings_path = dir.join("settings.json");
            let settings = load_settings(&settings_path);

            // 应用上次窗口尺寸（FR-1 记忆）
            if let (Some(w), Some(h)) = (settings.window_width, settings.window_height) {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_size(tauri::PhysicalSize::new(w, h));
                }
            }

            let state = Arc::new(AppState {
                timer: Mutex::new(Timer::new()),
                store: Mutex::new(store),
                start_at: Mutex::new(None),
                settings: Mutex::new(settings),
                settings_path,
                last_size_save: Mutex::new(Instant::now()),
            });
            app.manage(state.clone());
            apply_window_effects(app.handle());
            setup_tray(app.handle())?;
            // 开机自启（FR-6 默认开启；debug 下跳过，避免把调试 exe 写进注册表）
            if !cfg!(debug_assertions) {
                let autostart = state.settings.lock().unwrap().autostart;
                if autostart {
                    let _ = app.autolaunch().enable();
                } else {
                    let _ = app.autolaunch().disable();
                }
            }
            // 自启时以 --minimized 启动则缩到托盘
            if std::env::args().any(|a| a == "--minimized") {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            start_tick_thread(app.handle().clone(), state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            start_timer,
            pause_timer,
            resume_timer,
            reset_timer,
            finish_timer,
            get_stats,
            minimize_to_tray,
            quit_app,
            get_settings,
            set_autostart,
            start_drag,
            start_resize_dragging
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Resized(size) = event {
                let app = window.app_handle();
                let state = app.state::<Arc<AppState>>();
                let mut last = state.last_size_save.lock().unwrap();
                if last.elapsed() >= Duration::from_secs(1) {
                    *last = Instant::now();
                    let mut settings = state.settings.lock().unwrap();
                    settings.window_width = Some(size.width);
                    settings.window_height = Some(size.height);
                    let _ = save_settings(&state.settings_path, &settings);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> Arc<AppState> {
        let path = std::env::temp_dir().join(format!(
            "focus-timer-main-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let settings_path = path.with_extension("settings.json");
        Arc::new(AppState {
            timer: Mutex::new(Timer::new()),
            store: Mutex::new(Store::new(path)),
            start_at: Mutex::new(Some(Local::now())),
            settings: Mutex::new(Settings::default()),
            settings_path,
            last_size_save: Mutex::new(Instant::now()),
        })
    }

    #[test]
    fn record_finish_persists_natural_session() {
        let state = test_state();
        {
            let mut t = state.timer.lock().unwrap();
            t.start("写代码".into(), 100).unwrap();
            t.elapse(100).unwrap();
        }
        record_finish(&state, true).unwrap();
        let store = state.store.lock().unwrap();
        assert_eq!(store.sessions().len(), 1);
        assert_eq!(store.sessions()[0].task, "写代码");
        assert_eq!(store.sessions()[0].duration_secs, 100);
        assert_eq!(
            store.sessions()[0].date,
            Local::now().format("%Y-%m-%d").to_string()
        );
    }

    #[test]
    fn record_finish_persists_early_session() {
        let state = test_state();
        {
            let mut t = state.timer.lock().unwrap();
            t.start("写周报".into(), 1800).unwrap();
            t.elapse(600).unwrap();
        }
        record_finish(&state, false).unwrap();
        let store = state.store.lock().unwrap();
        assert_eq!(store.sessions().len(), 1);
        assert_eq!(store.sessions()[0].duration_secs, 600);
        assert_eq!(store.sessions()[0].task, "写周报");
    }
}
