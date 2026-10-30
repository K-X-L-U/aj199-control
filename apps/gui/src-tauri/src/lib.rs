//! Tauri-бэкенд AJAZZ AJ199.
//!
//! Все обращения к мыши идут через фоновый исполнитель (`session`): он держит
//! донгл открытым, выполняет задания по очереди и хранит профиль в памяти.
//! Перед каждой записью создаётся JSON-backup (из памяти, без чтения мыши).

// Tauri передаёт аргументы команд по значению, это контракт его макроса.
#![allow(clippy::needless_pass_by_value)]

mod power;
mod session;

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aj199_device::{DeviceSet, Profile};
use aj199_protocol::{KeyAction, SoftwareMacro, SoftwareMacroEvent};
use power::{Alert, ICON_SIZE, PowerState, Reading};
use serde::Serialize;
use serde_json::{Value, json};
use session::{Device, Job, Session, Worker};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Manager, RunEvent, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

const POWER_POLL_SECONDS: u32 = 30;

struct AppState {
    write_enabled: AtomicBool,
    quitting: AtomicBool,
    jobs: Sender<Job>,
    tray_snapshot: Mutex<TraySnapshot>,
    power: Mutex<PowerState>,
}

impl AppState {
    fn new() -> Self {
        Self {
            write_enabled: AtomicBool::new(false),
            quitting: AtomicBool::new(false),
            jobs: session::spawn(),
            tray_snapshot: Mutex::new(TraySnapshot::default()),
            power: Mutex::new(PowerState::default()),
        }
    }
}

/// Отправляет задание исполнителю и ждёт результат (в потоке команды,
/// не в главном потоке).
fn run_job<R: Send + 'static>(
    state: &AppState,
    job: impl FnOnce(&mut Worker) -> Result<R, String> + Send + 'static,
) -> Result<R, String> {
    let (reply, result) = mpsc::channel();
    state
        .jobs
        .send(Box::new(move |worker| {
            let _ = reply.send(job(worker));
        }))
        .map_err(|_| "исполнитель команд остановлен".to_string())?;
    result
        .recv()
        .map_err(|_| "исполнитель команд остановлен".to_string())?
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TraySnapshot {
    connected: bool,
    connection: String,
    battery: u8,
    charging: bool,
    dpi_x: u16,
    dpi_y: u16,
    stage: u8,
    stage_count: u8,
    write_enabled: bool,
}

impl TraySnapshot {
    fn from_state(dto: &StateDto) -> Self {
        let (dpi_x, dpi_y, stage, stage_count) =
            dto.profile.as_ref().map_or((0, 0, 0, 0), |profile| {
                let active = profile.active_dpi();
                (
                    active.map_or(0, |dpi| dpi.x),
                    active.map_or(0, |dpi| dpi.y),
                    profile.current_dpi_stage,
                    profile.dpi_stage_count,
                )
            });
        Self {
            connected: dto.connected,
            connection: dto.connection.clone(),
            battery: dto.battery,
            charging: dto.charging,
            dpi_x,
            dpi_y,
            stage,
            stage_count,
            write_enabled: dto.write_enabled,
        }
    }

    fn summary(&self) -> String {
        if !self.connected {
            return "AJAZZ AJ199 · мышь спит".to_string();
        }
        format!(
            "AJAZZ AJ199 · {} · {}%{}",
            self.connection,
            self.battery,
            if self.charging {
                " · зарядка"
            } else {
                ""
            }
        )
    }
}

#[derive(Debug, Serialize)]
struct StateDto {
    connected: bool,
    error: Option<String>,
    connection: String,
    vendor_id: u16,
    pid: u16,
    firmware: u16,
    cid: u8,
    mid: u8,
    battery: u8,
    charging: bool,
    write_enabled: bool,
    profile: Option<Profile>,
    backup: Option<String>,
}

#[derive(Debug, Serialize)]
struct StatusDto {
    connected: bool,
    error: Option<String>,
    connection: String,
    pid: u16,
    firmware: u16,
    battery: u8,
    charging: bool,
    write_enabled: bool,
}

#[derive(Debug, Serialize)]
struct MacroDto {
    index: usize,
    name: Option<String>,
    assigned_slot: Option<usize>,
}

fn create_main_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    let mut builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("AJAZZ AJ199")
        .inner_size(1200.0, 800.0)
        .min_inner_size(980.0, 640.0)
        .resizable(true)
        .decorations(false)
        .shadow(true)
        .visible(false)
        .devtools(cfg!(debug_assertions));
    if let Ok(dir) = app.path().app_local_data_dir() {
        builder = builder.data_directory(dir.join("WebView2"));
    }
    builder.build().map_err(|err| err.to_string())
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    match create_main_window(app) {
        Ok(window) => {
            let _ = window.show();
            let _ = window.set_focus();
        }
        Err(error) => eprintln!("не удалось создать окно: {error}"),
    }
}

fn build_tray_menu(app: &AppHandle, snapshot: &TraySnapshot) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    let header = MenuItem::with_id(app, "header", snapshot.summary(), false, None::<&str>)?;
    menu.append(&header)?;

    let dpi_text = if snapshot.connected {
        format!(
            "DPI {}×{} · ступень {}/{}",
            snapshot.dpi_x,
            snapshot.dpi_y,
            snapshot.stage + 1,
            snapshot.stage_count.max(1)
        )
    } else {
        "DPI: нет данных".to_string()
    };
    let dpi = MenuItem::with_id(app, "dpi", dpi_text, false, None::<&str>)?;
    menu.append(&dpi)?;

    if snapshot.connected && snapshot.stage_count > 0 {
        let submenu = Submenu::with_id(app, "stages", "Ступень DPI", snapshot.write_enabled)?;
        for index in 0..snapshot.stage_count.min(8) {
            let item = CheckMenuItem::with_id(
                app,
                format!("stage:{index}"),
                format!("Ступень {}", index + 1),
                snapshot.write_enabled,
                index == snapshot.stage,
                None::<&str>,
            )?;
            submenu.append(&item)?;
        }
        menu.append(&submenu)?;
    }

    let profile = MenuItem::with_id(
        app,
        "profile",
        "Профиль: переключение не проверено",
        false,
        None::<&str>,
    )?;
    menu.append(&profile)?;

    let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "Автозапуск",
        true,
        autostart_enabled,
        None::<&str>,
    )?;
    menu.append(&autostart)?;

    let show = MenuItem::with_id(app, "show", "Открыть конфигуратор", true, None::<&str>)?;
    menu.append(&show)?;
    let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
    menu.append(&quit)?;
    Ok(menu)
}

fn update_tray_menu(app: &AppHandle, snapshot: &TraySnapshot) {
    if let Some(tray) = app.tray_by_id("main") {
        if let Ok(menu) = build_tray_menu(app, snapshot) {
            let _ = tray.set_menu(Some(menu));
        }
        let _ = tray.set_tooltip(Some(snapshot.summary()));
    }
}

fn remember_state(app: &AppHandle, state: &StateDto) {
    let snapshot = TraySnapshot::from_state(state);
    if let Ok(mut cached) = app.state::<AppState>().tray_snapshot.lock() {
        *cached = snapshot.clone();
    }
    update_tray_menu(app, &snapshot);
}

fn tray_set_stage(app: &AppHandle, stage: u8) {
    let app_state = app.state::<AppState>();
    if !app_state.write_enabled.load(Ordering::SeqCst) {
        return;
    }
    if let Err(error) = with_write(app, &app_state, move |device, _profile| {
        device
            .set_current_dpi_stage(stage)
            .map_err(|err| err.to_string())
    }) {
        eprintln!("tray set stage: {error}");
    }
}

fn toggle_autostart(app: &AppHandle) {
    let autolaunch = app.autolaunch();
    let enabled = autolaunch.is_enabled().unwrap_or(false);
    let result = if enabled {
        autolaunch.disable()
    } else {
        autolaunch.enable()
    };
    if let Err(error) = result {
        eprintln!("autostart: {error}");
    }
    let snapshot = app
        .state::<AppState>()
        .tray_snapshot
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default();
    update_tray_menu(app, &snapshot);
}

fn state_dto(
    session: &Session,
    profile: Profile,
    backup: Option<String>,
    write_enabled: bool,
) -> StateDto {
    let set = &session.set;
    StateDto {
        connected: true,
        error: None,
        connection: set.connection.label().to_string(),
        vendor_id: set.command.vendor_id,
        pid: set.command.product_id,
        firmware: set.command.release_number,
        cid: session.cid,
        mid: session.mid,
        battery: session.battery,
        charging: session.charging,
        write_enabled,
        profile: Some(profile),
        backup,
    }
}

fn disconnected(error: String, write_enabled: bool) -> StateDto {
    StateDto {
        connected: false,
        error: Some(error),
        connection: "offline".to_string(),
        vendor_id: 0,
        pid: 0,
        firmware: 0,
        cid: 0,
        mid: 0,
        battery: 0,
        charging: false,
        write_enabled,
        profile: None,
        backup: None,
    }
}

fn save_macro_backup(
    app: &AppHandle,
    index: usize,
    macro_data: &SoftwareMacro,
) -> Result<String, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| err.to_string())?
        .join("backups")
        .join("macros");
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let path = dir.join(format!("macro_{index}_{}.json", unix_now()?));
    let dump = json!({
        "metadata": {
            "generated_at_unix": unix_now()?,
            "macro_index": index,
        },
        "macro": macro_data,
    });
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&dump).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

fn save_backup(app: &AppHandle, set: &DeviceSet, profile: &Profile) -> Result<String, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| err.to_string())?
        .join("backups");
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let path = dir.join(format!("backup_{}.json", unix_now()?));
    let dump = json!({
        "metadata": {
            "generated_at_unix": unix_now()?,
            "connection": set.connection.label(),
            "vendor_id": set.command.vendor_id,
            "pid": set.command.product_id,
            "firmware": set.command.release_number,
            "command_path": set.command.path_display(),
            "notify_path": set.notify.path_display(),
        },
        "profile": profile,
        "raw_hex": profile.raw.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" "),
    });
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&dump).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

/// Запись: профиль берётся из памяти, после записи кэш обновляется
/// отправленными байтами — мышь целиком не перечитывается.
fn with_write<F>(app: &AppHandle, state: &AppState, action: F) -> Result<StateDto, String>
where
    F: FnOnce(&mut Device, &Profile) -> Result<(), String> + Send + 'static,
{
    if !state.write_enabled.load(Ordering::SeqCst) {
        return Err("Запись недоступна: мышь спит или окно программы закрыто.".to_string());
    }
    let app = app.clone();
    let tray_app = app.clone();
    let snapshot = run_job(state, move |worker| {
        worker
            .write(|session, profile| {
                let backup = save_backup(&app, &session.set, profile)?;
                action(&mut session.device, profile)?;
                Ok(backup)
            })
            .and_then(|backup| {
                worker.read(|session| {
                    let profile = session.profile()?;
                    Ok(state_dto(session, profile, Some(backup), true))
                })
            })
    })?;
    remember_state(&tray_app, &snapshot);
    Ok(snapshot)
}

#[tauri::command(async)]
fn get_state(app: AppHandle) -> StateDto {
    let state = app.state::<AppState>();
    let enabled = state.write_enabled.load(Ordering::SeqCst);
    let result = run_job(&state, move |worker| {
        let profile = worker.reconnect_resync()?;
        worker.read(|session| Ok(state_dto(session, profile, None, enabled)))
    });
    match result {
        Ok(mut snapshot) => {
            snapshot.write_enabled = set_write_mode(&app, true);
            remember_state(&app, &snapshot);
            snapshot
        }
        Err(error) => disconnected(error, set_write_mode(&app, false)),
    }
}

#[tauri::command(async)]
fn get_status(app: AppHandle) -> StatusDto {
    let state = app.state::<AppState>();
    let probe = session::probe_connection();
    let result = run_job(&state, move |worker| {
        worker.sync_connection(probe);
        worker.read(|session| {
            let awake = session.power_tick().is_some();
            Ok((
                awake,
                StatusDto {
                    connected: true,
                    error: None,
                    connection: session.set.connection.label().to_string(),
                    pid: session.set.command.product_id,
                    firmware: session.set.command.release_number,
                    battery: session.battery,
                    charging: session.charging,
                    write_enabled: false,
                },
            ))
        })
    });
    match result {
        Ok((awake, status)) => StatusDto {
            write_enabled: set_write_mode(&app, awake),
            ..status
        },
        Err(error) => StatusDto {
            connected: false,
            error: Some(error),
            connection: "offline".to_string(),
            pid: 0,
            firmware: 0,
            battery: 0,
            charging: false,
            write_enabled: set_write_mode(&app, false),
        },
    }
}

/// Запись разрешена, только пока окно открыто и мышь отвечает.
fn set_write_mode(app: &AppHandle, awake: bool) -> bool {
    let state = app.state::<AppState>();
    let enabled = awake && app.get_webview_window("main").is_some();
    if state.write_enabled.swap(enabled, Ordering::SeqCst) != enabled {
        let snapshot = state.tray_snapshot.lock().ok().map(|mut value| {
            value.write_enabled = enabled;
            value.clone()
        });
        if let Some(snapshot) = snapshot {
            update_tray_menu(app, &snapshot);
        }
    }
    enabled
}

#[tauri::command(async)]
fn backup(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    run_job(&state, move |worker| {
        worker.read(|session| {
            let profile = session.profile()?;
            save_backup(&app, &session.set, &profile)
        })
    })
}

#[tauri::command(async)]
fn export_profile(state: State<'_, AppState>) -> Result<String, String> {
    run_job(&state, |worker| {
        worker.read(|session| {
            let profile = session.profile()?;
            serde_json::to_string_pretty(&profile).map_err(|err| err.to_string())
        })
    })
}

#[tauri::command(async)]
fn restore_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    json: String,
) -> Result<StateDto, String> {
    let value: Value = serde_json::from_str(&json).map_err(|err| err.to_string())?;
    let profile_value = value.get("profile").cloned().unwrap_or(value);
    let profile: Profile =
        serde_json::from_value(profile_value).map_err(|err| format!("неверный профиль: {err}"))?;
    with_write(&app, &state, move |device, _old| {
        device
            .apply_known_profile(&profile)
            .map_err(|err| err.to_string())
    })
}

#[tauri::command(async)]
fn get_macros(state: State<'_, AppState>) -> Result<Vec<MacroDto>, String> {
    run_job(&state, |worker| {
        worker.read(|session| {
            let mut result = Vec::new();
            for index in 0..16 {
                let name = session
                    .device
                    .read_macro_name(index)
                    .map_err(|err| err.to_string())?;
                if let Some(name) = name {
                    result.push(MacroDto {
                        index,
                        name: Some(name),
                        assigned_slot: None,
                    });
                }
            }
            Ok(result)
        })
    })
}

#[tauri::command(async)]
fn set_macro_name(
    app: AppHandle,
    state: State<'_, AppState>,
    index: usize,
    name: String,
) -> Result<StateDto, String> {
    let backup_app = app.clone();
    with_write(&app, &state, move |device, _profile| {
        let app = backup_app;
        // Мышь не принимает запись одного имени: слот пишется целиком
        // (имя + счётчик + события + checksum), как при создании.
        let events = match device.read_macro(index) {
            Ok(old) => {
                save_macro_backup(&app, index, &old)?;
                old.events
            }
            Err(_) => Vec::new(),
        };
        let macro_data = SoftwareMacro {
            name: name.trim().to_string(),
            events,
        };
        write_macro_slot(device, index, &macro_data)
    })
}

#[tauri::command(async)]
fn read_macro(state: State<'_, AppState>, index: usize) -> Result<SoftwareMacro, String> {
    run_job(&state, move |worker| {
        worker.read(|session| {
            let count = session
                .device
                .read_eeprom(macro_base(index) + 0x1F, 1)
                .map_err(|err| err.to_string())?[0];
            if count == 0xFF {
                return Ok(SoftwareMacro {
                    name: String::new(),
                    events: Vec::new(),
                });
            }
            session
                .device
                .read_macro(index)
                .map_err(|err| err.to_string())
        })
    })
}

const MACRO_SLOTS: u16 = 16;

fn macro_base(index: usize) -> u16 {
    let slot = u16::try_from(index).map_or(MACRO_SLOTS - 1, |slot| slot.min(MACRO_SLOTS - 1));
    0x0300 + slot * 0x0180
}

fn is_macro_key(code: u32, index: usize) -> bool {
    code & 0xFF == 0x06 && u32::try_from(index) == Ok((code >> 8) & 0xFF)
}

fn write_macro_slot(
    device: &mut Device,
    index: usize,
    macro_data: &SoftwareMacro,
) -> Result<(), String> {
    let data = encode_macro_slot(macro_data)?;
    device
        .write_verified(macro_base(index), &data)
        .map_err(|err| err.to_string())
}

fn encode_macro_slot(macro_data: &SoftwareMacro) -> Result<Vec<u8>, String> {
    let mut data = macro_data
        .encode_hardware_slot()
        .map_err(|err| err.to_string())?;
    let used = 32 + macro_data.events.len() * 5;
    let sum = data[31..used]
        .iter()
        .fold(0u8, |acc, &byte| acc.wrapping_add(byte));
    data[used] = 0x55u8.wrapping_sub(sum);
    data.truncate((used + 4).min(0x180));
    Ok(data)
}

#[tauri::command(async)]
fn write_macro(
    app: AppHandle,
    state: State<'_, AppState>,
    index: usize,
    name: String,
    events: Vec<SoftwareMacroEvent>,
) -> Result<StateDto, String> {
    let macro_data = SoftwareMacro {
        name: name.trim().to_string(),
        events,
    };
    let backup_app = app.clone();
    with_write(&app, &state, move |device, _profile| {
        if let Ok(old) = device.read_macro(index) {
            save_macro_backup(&backup_app, index, &old)?;
        }
        write_macro_slot(device, index, &macro_data)
    })
}

#[tauri::command]
fn encode_jmm(name: String, events: Vec<SoftwareMacroEvent>) -> Result<Vec<u8>, String> {
    SoftwareMacro {
        name: name.trim().to_string(),
        events,
    }
    .to_jmm()
    .map_err(|err| err.to_string())
}

#[tauri::command]
fn parse_jmm(bytes: Vec<u8>) -> Result<SoftwareMacro, String> {
    SoftwareMacro::parse_jmm(&bytes).map_err(|err| err.to_string())
}

#[tauri::command(async)]
fn find_empty_macro_slot(state: State<'_, AppState>) -> Result<Option<usize>, String> {
    run_job(&state, |worker| {
        worker.read(|session| {
            let keys = session.profile()?.keys;
            let assigned = |index: usize| keys.iter().any(|key| is_macro_key(key.code, index));
            let device = &mut session.device;
            for index in 0..16 {
                if assigned(index) {
                    continue;
                }
                if device
                    .read_macro_name(index)
                    .map_err(|err| err.to_string())?
                    .is_some()
                {
                    continue;
                }
                let count = device
                    .read_eeprom(macro_base(index) + 0x1F, 1)
                    .map_err(|err| err.to_string())?[0];
                // 0xFF — стёртый слот, 0x00 — очищенный нами.
                if count == 0 || count == 0xFF {
                    return Ok(Some(index));
                }
            }
            Ok(None)
        })
    })
}

// Заводская раскладка (startup_read_all); кнопку 6 OEM при удалении макроса
// возвращает в DPI loop (macro_delete_dongle).
fn default_key_code(slot: usize) -> u32 {
    const DEFAULTS: [u32; 6] = [0x0101, 0x1001, 0x0401, 0x0801, 0x0201, 0x0102];
    DEFAULTS.get(slot).copied().unwrap_or(0)
}

#[tauri::command(async)]
fn delete_macro(
    app: AppHandle,
    state: State<'_, AppState>,
    index: usize,
) -> Result<StateDto, String> {
    let backup_app = app.clone();
    with_write(&app, &state, move |device, profile| {
        if let Ok(old) = device.read_macro(index) {
            save_macro_backup(&backup_app, index, &old)?;
        }
        for (slot, key) in profile.keys.iter().enumerate() {
            if is_macro_key(key.code, index) {
                device
                    .set_key_raw(slot, default_key_code(slot))
                    .map_err(|err| err.to_string())?;
            }
        }
        let mut data = [0u8; 36];
        data[32] = 0x55;
        device
            .write_verified(macro_base(index), &data)
            .map_err(|err| err.to_string())
    })
}

#[tauri::command(async)]
fn set_dpi(
    app: AppHandle,
    app_state: State<'_, AppState>,
    stage: usize,
    x: u16,
    y: u16,
) -> Result<StateDto, String> {
    with_write(&app, &app_state, move |device, _profile| {
        device
            .set_dpi_stage(stage, x, y)
            .map(|_| ())
            .map_err(|err| err.to_string())
    })
}

const DPI_STAGE_LIMIT: u8 = 6;

fn write_stage_count(device: &mut Device, count: u8) -> Result<(), String> {
    device
        .write_verified(
            aj199_protocol::addr::DPI_STAGE_COUNT,
            &aj199_protocol::pair(count),
        )
        .map_err(|err| err.to_string())
}

#[tauri::command(async)]
fn add_dpi_stage(app: AppHandle, state: State<'_, AppState>) -> Result<StateDto, String> {
    with_write(&app, &state, move |device, profile| {
        let count = profile.dpi_stage_count.clamp(1, DPI_STAGE_LIMIT);
        if count >= DPI_STAGE_LIMIT {
            return Err(format!("Максимум {DPI_STAGE_LIMIT} ступеней"));
        }
        write_stage_count(device, count + 1)
    })
}

#[tauri::command(async)]
fn delete_dpi_stage(
    app: AppHandle,
    state: State<'_, AppState>,
    index: usize,
) -> Result<StateDto, String> {
    with_write(&app, &state, move |device, profile| {
        let count = profile.dpi_stage_count.clamp(1, DPI_STAGE_LIMIT);
        if count <= 1 {
            return Err("Нельзя удалить единственную ступень".to_string());
        }
        let index_u8 = u8::try_from(index)
            .ok()
            .filter(|index| *index < count)
            .ok_or_else(|| format!("Ступень {} неактивна", index + 1))?;
        let new_count = count - 1;
        let current = profile.current_dpi_stage;
        let new_current = if current > index_u8 {
            current - 1
        } else {
            current.min(new_count - 1)
        };
        if new_current != current {
            device
                .set_current_dpi_stage(new_current)
                .map_err(|err| err.to_string())?;
        }
        for stage in index..usize::from(new_count) {
            let next = &profile.dpi_stages[stage + 1];
            device
                .set_dpi_stage(stage, next.x, next.y)
                .map_err(|err| err.to_string())?;
            if let Some(color) = profile.dpi_colors.get(stage + 1) {
                device
                    .set_dpi_color(stage, color.r, color.g, color.b)
                    .map_err(|err| err.to_string())?;
            }
        }
        write_stage_count(device, new_count)
    })
}

#[tauri::command(async)]
fn set_current_stage(
    app: AppHandle,
    app_state: State<'_, AppState>,
    stage: u8,
) -> Result<StateDto, String> {
    with_write(&app, &app_state, move |device, _profile| {
        device
            .set_current_dpi_stage(stage)
            .map_err(|err| err.to_string())
    })
}

#[tauri::command(async)]
fn test_write(app: AppHandle, state: State<'_, AppState>) -> Result<StateDto, String> {
    with_write(&app, &state, move |device, profile| {
        device
            .set_debounce(profile.debounce)
            .map_err(|err| err.to_string())
    })
}

#[tauri::command(async)]
fn set_dpi_color(
    app: AppHandle,
    app_state: State<'_, AppState>,
    stage: usize,
    r: u8,
    g: u8,
    b: u8,
) -> Result<StateDto, String> {
    with_write(&app, &app_state, move |device, _profile| {
        device
            .set_dpi_color(stage, r, g, b)
            .map_err(|err| err.to_string())
    })
}

#[tauri::command(async)]
fn set_polling(app: AppHandle, state: State<'_, AppState>, hz: u16) -> Result<StateDto, String> {
    with_write(&app, &state, move |device, _profile| {
        device.set_polling_hz(hz).map_err(|err| err.to_string())
    })
}

#[tauri::command(async)]
fn set_debounce(app: AppHandle, state: State<'_, AppState>, value: u8) -> Result<StateDto, String> {
    with_write(&app, &state, move |device, _profile| {
        device.set_debounce(value).map_err(|err| err.to_string())
    })
}

#[tauri::command(async)]
fn set_lod(app: AppHandle, state: State<'_, AppState>, value: u8) -> Result<StateDto, String> {
    if !(1..=2).contains(&value) {
        return Err("LOD: допустимо 1 или 2".to_string());
    }
    with_write(&app, &state, move |device, _profile| {
        device.set_lod(value).map_err(|err| err.to_string())
    })
}

#[tauri::command(async)]
fn set_angle_snap(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<StateDto, String> {
    with_write(&app, &state, move |device, _profile| {
        device
            .set_angle_snap(enabled)
            .map_err(|err| err.to_string())
    })
}

#[tauri::command(async)]
fn set_ripple(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<StateDto, String> {
    with_write(&app, &state, move |device, _profile| {
        device.set_ripple(enabled).map_err(|err| err.to_string())
    })
}

#[tauri::command(async)]
fn set_led(
    app: AppHandle,
    state: State<'_, AppState>,
    mode: u8,
    speed: u8,
    brightness: u8,
) -> Result<StateDto, String> {
    // В OEM-прошивке три режима: 0 off, 1 steady, 2 breathing. Значения
    // байтов пишутся как есть: у steady speed-байт — яркость до 0xFF,
    // у breathing brightness-байт — скорость 1..5 (clone ограничивает его 3).
    if mode > 2 {
        return Err(format!("неизвестный режим подсветки: {mode}"));
    }
    with_write(&app, &state, move |device, _profile| {
        device
            .write_verified(
                aj199_protocol::addr::MAIN_LED,
                &aj199_protocol::led_state(mode, speed, brightness),
            )
            .map_err(|err| err.to_string())
    })
}

#[tauri::command(async)]
fn set_key(
    app: AppHandle,
    state: State<'_, AppState>,
    slot: usize,
    action: Option<String>,
    code: Option<u32>,
) -> Result<StateDto, String> {
    let new_code = match (action, code) {
        (Some(action), None) => KeyAction::from_name(&action)
            .ok_or_else(|| format!("неизвестное действие: {action}"))?
            .hardware_code(),
        (None, Some(code)) => code,
        _ => return Err("укажи одно: action или code".to_string()),
    };
    with_write(&app, &state, move |device, profile| {
        if slot >= profile.keys.len() {
            return Err("неверный слот кнопки".to_string());
        }
        let left_count = profile.keys.iter().filter(|key| key.code == 0x0101).count();
        if profile.keys[slot].code == 0x0101 && new_code != 0x0101 && left_count <= 1 {
            return Err("Хотя бы одна кнопка должна оставаться левой".to_string());
        }
        device
            .set_key_raw(slot, new_code)
            .map_err(|err| err.to_string())
    })
}

#[tauri::command(async)]
fn reset_profile(app: AppHandle, state: State<'_, AppState>) -> Result<StateDto, String> {
    with_write(&app, &state, move |device, _profile| {
        device.reset_profile().map_err(|err| err.to_string())
    })
}

fn settings_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|err| err.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    Ok(dir.join("settings.json"))
}

fn read_settings(app: &AppHandle) -> Value {
    settings_path(app)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

/// Файл пишется сразу: `localStorage` `WebView2` сохраняется с задержкой
/// и теряется при быстром выходе. Запись через временный файл.
fn write_setting(app: &AppHandle, key: &str, setting: Value) -> Result<(), String> {
    let path = settings_path(app)?;
    let mut value = read_settings(app);
    value[key] = setting;
    let temp = path.with_extension("json.tmp");
    std::fs::write(
        &temp,
        serde_json::to_vec_pretty(&value).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    std::fs::rename(&temp, &path).map_err(|err| err.to_string())
}

#[tauri::command]
fn get_theme(app: AppHandle) -> Option<String> {
    read_settings(&app)
        .get("theme")
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[tauri::command]
fn set_theme(app: AppHandle, state: State<'_, AppState>, theme: String) -> Result<(), String> {
    write_setting(&app, "theme", json!(theme))?;
    if let Ok(mut power) = state.power.lock() {
        power.theme = theme;
    }
    Ok(())
}

fn art_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| err.to_string())?
        .join("toast");
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    Ok(dir)
}

/// Картинки уведомлений рисует интерфейс: окна может не быть, когда придёт время.
#[tauri::command(async)]
fn save_theme_art(
    app: AppHandle,
    theme: String,
    logo: Vec<u8>,
    low: Vec<u8>,
    full: Vec<u8>,
) -> Result<(), String> {
    if !theme.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("неверное имя темы".to_string());
    }
    let dir = art_dir(&app)?;
    for (name, data) in [("logo", logo), ("low", low), ("full", full)] {
        std::fs::write(dir.join(format!("{theme}-{name}.png")), data)
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn icon_cache_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(art_dir(app)?.join("tray-icon.rgba"))
}

/// Иконка трея: батарея, пока мышь отвечает, иначе просто значок темы.
fn refresh_tray_icon(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Ok(power) = state.power.lock() else {
        return;
    };
    let pixels = power.reading.map_or_else(
        || power.theme_icon.clone(),
        |reading| Some(power::draw_battery_icon(&power.theme, reading)),
    );
    drop(power);
    if let (Some(pixels), Some(tray)) = (pixels, app.tray_by_id("main")) {
        let image = tauri::image::Image::new_owned(pixels, ICON_SIZE, ICON_SIZE);
        let _ = tray.set_icon(Some(image));
    }
}

/// Значок темы для окна; в трей он попадает, только если мышь спит.
#[tauri::command]
fn set_theme_icon(
    app: AppHandle,
    state: State<'_, AppState>,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
) -> Result<(), String> {
    if width != ICON_SIZE || height != ICON_SIZE || rgba.len() != (width * height * 4) as usize {
        return Err("неверный размер иконки".to_string());
    }
    if let Some(window) = app.get_webview_window("main") {
        let image = tauri::image::Image::new_owned(rgba.clone(), width, height);
        window.set_icon(image).map_err(|err| err.to_string())?;
    }
    if let Ok(path) = icon_cache_path(&app) {
        let _ = std::fs::write(path, &rgba);
    }
    if let Ok(mut power) = state.power.lock() {
        power.theme_icon = Some(rgba);
    }
    refresh_tray_icon(&app);
    Ok(())
}

fn toast_now(app: &AppHandle, alert: Alert, battery: u8) -> Result<(), String> {
    let theme = app
        .state::<AppState>()
        .power
        .lock()
        .map(|power| power.theme.clone())
        .unwrap_or_default();
    power::show_toast(&theme, alert, battery, &art_dir(app)?)
}

#[tauri::command]
fn get_battery_alerts(state: State<'_, AppState>) -> bool {
    state
        .power
        .lock()
        .map_or(true, |power| power.alerts_enabled)
}

#[tauri::command]
fn set_battery_alerts(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    write_setting(&app, "battery_alerts", json!(enabled))?;
    if let Ok(mut power) = state.power.lock() {
        power.alerts_enabled = enabled;
    }
    Ok(())
}

#[tauri::command(async)]
fn test_battery_toast(app: AppHandle, kind: String) -> Result<(), String> {
    let (alert, battery) = match kind.as_str() {
        "critical" => (Alert::Critical, 10),
        "full" => (Alert::Full, 100),
        _ => (Alert::Low, 20),
    };
    toast_now(&app, alert, battery)
}

/// Результат проверки: снимок для меню трея или `None`, если мышь спит.
fn apply_tick(app: &AppHandle, tick: Option<TraySnapshot>) {
    let state = app.state::<AppState>();
    let write_enabled = set_write_mode(app, tick.is_some());
    let reading = tick.as_ref().map(|snapshot| Reading {
        battery: snapshot.battery,
        charging: snapshot.charging,
    });
    let (icon_changed, alert) = {
        let Ok(mut power) = state.power.lock() else {
            return;
        };
        let changed = power.reading != reading;
        power.reading = reading;
        let alert = match reading {
            Some(reading) if power.alerts_enabled => power.alerts.evaluate(reading),
            _ => None,
        };
        (changed, alert)
    };
    if icon_changed {
        refresh_tray_icon(app);
    }
    let next = tick.unwrap_or_else(|| {
        let mut asleep = state
            .tray_snapshot
            .lock()
            .map(|cached| cached.clone())
            .unwrap_or_default();
        asleep.connected = false;
        asleep
    });
    let next = TraySnapshot {
        write_enabled,
        ..next
    };
    let menu_changed = state.tray_snapshot.lock().is_ok_and(|mut cached| {
        let changed = *cached != next;
        cached.clone_from(&next);
        changed
    });
    if menu_changed {
        update_tray_menu(app, &next);
    }
    if let (Some(alert), Some(reading)) = (alert, reading)
        && let Err(error) = toast_now(app, alert, reading.battery)
    {
        eprintln!("уведомление: {error}");
    }
}

/// Работает и при закрытом окне: заряд в трее и уведомления.
fn power_worker(app: AppHandle) {
    {
        let settings = read_settings(&app);
        let state = app.state::<AppState>();
        if let Ok(mut power) = state.power.lock() {
            power.theme = settings
                .get("theme")
                .and_then(Value::as_str)
                .unwrap_or("deep")
                .to_string();
            power.alerts_enabled = settings
                .get("battery_alerts")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            power.theme_icon = icon_cache_path(&app)
                .ok()
                .and_then(|path| std::fs::read(path).ok())
                .filter(|data| data.len() == (ICON_SIZE * ICON_SIZE * 4) as usize);
        }
    }
    refresh_tray_icon(&app);
    loop {
        let state = app.state::<AppState>();
        if state.quitting.load(Ordering::SeqCst) {
            break;
        }
        let probe = session::probe_connection();
        let tick = run_job(&state, move |worker| {
            worker.sync_connection(probe);
            Ok(worker.session().ok().and_then(|session| {
                let (battery, charging) = session.power_tick()?;
                let profile = session.profile().ok()?;
                let active = profile.active_dpi();
                Some(TraySnapshot {
                    connected: true,
                    connection: session.set.connection.label().to_string(),
                    battery,
                    charging,
                    dpi_x: active.map_or(0, |dpi| dpi.x),
                    dpi_y: active.map_or(0, |dpi| dpi.y),
                    stage: profile.current_dpi_stage,
                    stage_count: profile.dpi_stage_count,
                    write_enabled: false,
                })
            }))
        })
        .ok()
        .flatten();
        apply_tick(&app, tick);
        for _ in 0..POWER_POLL_SECONDS {
            if state.quitting.load(Ordering::SeqCst) {
                return;
            }
            thread::sleep(Duration::from_secs(1));
        }
    }
}

fn unix_now() -> Result<u64, String> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| err.to_string())?
        .as_secs())
}

fn handle_tray_menu(app: &AppHandle, id: &str) {
    if id == "show" {
        show_main_window(app);
    } else if id == "quit" {
        app.state::<AppState>()
            .quitting
            .store(true, Ordering::SeqCst);
        app.exit(0);
    } else if id == "autostart" {
        toggle_autostart(app);
    } else if let Some(stage) = id.strip_prefix("stage:")
        && let Ok(stage) = stage.parse::<u8>()
    {
        tray_set_stage(app, stage);
    }
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_tray_menu(app, &TraySnapshot::default())?;
    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| handle_tray_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

fn handle_window_event(window: &tauri::Window, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }
    match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let _ = window.destroy();
        }
        WindowEvent::Resized(_) => {
            if matches!(window.is_minimized(), Ok(true)) {
                let _ = window.destroy();
            }
        }
        WindowEvent::Destroyed => {
            let app = window.app_handle();
            set_write_mode(app, false);
            let snapshot = app
                .state::<AppState>()
                .tray_snapshot
                .lock()
                .map(|value| value.clone())
                .unwrap_or_default();
            update_tray_menu(app, &snapshot);
        }
        _ => {}
    }
}

/// Запускает приложение.
///
/// # Panics
///
/// Если Tauri не смог создать приложение: без него работать нечему.
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            get_state,
            get_status,
            backup,
            export_profile,
            restore_profile,
            get_macros,
            set_macro_name,
            read_macro,
            write_macro,
            encode_jmm,
            parse_jmm,
            find_empty_macro_slot,
            delete_macro,
            set_dpi,
            set_current_stage,
            add_dpi_stage,
            delete_dpi_stage,
            test_write,
            set_dpi_color,
            set_polling,
            set_debounce,
            set_lod,
            set_angle_snap,
            set_ripple,
            set_led,
            set_key,
            reset_profile,
            set_theme_icon,
            get_theme,
            set_theme,
            save_theme_art,
            get_battery_alerts,
            set_battery_alerts,
            test_battery_toast
        ])
        .setup(|app| {
            setup_tray(app.handle())?;
            let handle = app.handle().clone();
            thread::spawn(move || power_worker(handle));
            if !std::env::args().any(|arg| arg == "--minimized") {
                show_main_window(app.handle());
            }
            Ok(())
        })
        .on_window_event(handle_window_event)
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let RunEvent::ExitRequested { api, .. } = event
            && !app_handle
                .state::<AppState>()
                .quitting
                .load(Ordering::SeqCst)
        {
            api.prevent_exit();
        }
    });
}
