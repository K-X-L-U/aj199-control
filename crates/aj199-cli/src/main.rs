//! CLI `aj199ctl`.
//!
//! Stage C: чтение состояния, backup, DPI-запись.
//! Остальные записи включим после соответствующих записей трафика.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aj199_device::{
    Aj199Device, ConnectionKind, DeviceSet, HidTransport, Profile, SimulatorTransport, Transport,
    TransportError,
};
use aj199_protocol::KeyAction;
use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

const SIM_PROFILE_HEX: &str = include_str!("../../../fixtures/profile_dongle.hex");

type BoxedDevice = Aj199Device<Box<dyn Transport>>;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SimulatorKind {
    Wired,
    Dongle,
}

#[derive(Debug, Parser)]
#[command(name = "aj199ctl", version, about = "AJAZZ AJ199 CLI")]
struct Cli {
    /// Работать без устройства, на записанном профиле.
    #[arg(long, global = true, value_enum)]
    simulator: Option<SimulatorKind>,

    /// Таймаут ожидания ответа, мс.
    #[arg(long, global = true, default_value_t = 2500)]
    timeout_ms: u64,

    /// Число повторов команды.
    #[arg(long, global = true, default_value_t = 3)]
    retries: usize,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Показать все HID-интерфейсы AJ199.
    ListInterfaces,
    /// Краткая информация об устройстве.
    Info,
    /// Заряд и статус зарядки.
    Battery,
    /// Прочитать весь профиль и сохранить JSON.
    DumpAll {
        #[arg(long)]
        out: PathBuf,
    },
    /// Сравнить два dump-all JSON.
    Diff { first: PathBuf, second: PathBuf },
    /// Получить настройки.
    Get {
        #[command(subcommand)]
        what: GetCommands,
    },
    /// Записать настройки.
    Set {
        #[command(subcommand)]
        what: SetCommands,
    },
    /// Читать уведомления устройства.
    Monitor {
        #[arg(long, default_value_t = 30)]
        seconds: u64,
    },
    /// Восстановить известные блоки из backup.
    RestoreBackup {
        backup: PathBuf,
        #[arg(long)]
        allow_write: bool,
    },
}

#[derive(Debug, Subcommand)]
enum GetCommands {
    Dpi,
    Led,
    Debounce,
    Lod,
    Polling,
    AngleSnap,
    Ripple,
    Keys,
    Profile,
}

#[derive(Debug, Args)]
struct WriteOptions {
    /// Разрешить запись в устройство.
    #[arg(long)]
    allow_write: bool,
    /// Куда сохранить backup (по умолчанию текущая папка).
    #[arg(long)]
    backup_dir: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum SetCommands {
    /// Записать DPI ступени (0..7).
    Dpi {
        #[arg(long)]
        stage: usize,
        #[arg(long)]
        x: u16,
        #[arg(long)]
        y: u16,
        #[command(flatten)]
        write: WriteOptions,
    },
    /// Записать debounce (0..20 мс).
    Debounce {
        #[arg(long)]
        value: u8,
        #[command(flatten)]
        write: WriteOptions,
    },
    /// Записать основной LED: mode, speed, brightness.
    Led {
        #[arg(long)]
        mode: u8,
        #[arg(long)]
        speed: u8,
        #[arg(long)]
        brightness: u8,
        #[command(flatten)]
        write: WriteOptions,
    },
    /// Записать цвет ступени DPI: R, G, B.
    DpiColor {
        #[arg(long)]
        stage: usize,
        #[arg(long)]
        r: u8,
        #[arg(long)]
        g: u8,
        #[arg(long)]
        b: u8,
        #[command(flatten)]
        write: WriteOptions,
    },
    /// Записать hardware-код кнопки (24 бита) в слот 0..15.
    Key {
        #[arg(long)]
        slot: usize,
        #[arg(long, value_parser = parse_u32)]
        code: Option<u32>,
        #[arg(long)]
        action: Option<String>,
        #[command(flatten)]
        write: WriteOptions,
    },
    /// Записать LOD (0..3).
    Lod {
        #[arg(long)]
        value: u8,
        #[command(flatten)]
        write: WriteOptions,
    },
    /// Записать частоту опроса: 125, 250, 500 или 1000 Гц.
    Polling {
        #[arg(long)]
        hz: u16,
        #[command(flatten)]
        write: WriteOptions,
    },
    /// Angle snap: 0 или 1.
    AngleSnap {
        #[arg(long)]
        value: u8,
        #[command(flatten)]
        write: WriteOptions,
    },
    /// Ripple control: 0 или 1.
    Ripple {
        #[arg(long)]
        value: u8,
        #[command(flatten)]
        write: WriteOptions,
    },
    /// Сбросить текущий профиль (команда 0x09).
    ProfileReset {
        #[command(flatten)]
        write: WriteOptions,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct Dump {
    metadata: Metadata,
    profile: Profile,
    raw_hex: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Metadata {
    generated_at_unix: u64,
    connection: String,
    vendor_id: u16,
    pid: u16,
    product: Option<String>,
    release_number: u16,
    command_path: String,
    notify_path: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Commands::ListInterfaces => list_interfaces(),
        Commands::Info => info(&cli),
        Commands::Battery => battery(&cli),
        Commands::DumpAll { out } => dump_all(&cli, out),
        Commands::Diff { first, second } => diff(first, second),
        Commands::Get { what } => get(&cli, what),
        Commands::Set { what } => set(&cli, what),
        Commands::Monitor { seconds } => monitor(&cli, *seconds),
        Commands::RestoreBackup {
            backup,
            allow_write,
        } => restore_backup(&cli, backup, *allow_write),
    }
}

fn list_interfaces() -> Result<()> {
    let transport = HidTransport::new()?;
    let interfaces = transport.list_interfaces()?;
    if interfaces.is_empty() {
        bail!("AJ199 interfaces not found");
    }
    println!(
        "{:>6}  {:>4}  {:>6}  {:>6}  {:>6}  PATH",
        "PID", "IF", "PAGE", "USAGE", "VER"
    );
    for interface in interfaces {
        println!(
            "0x{:04X}  {:>4}  0x{:04X}  0x{:04X}  0x{:04X}  {}",
            interface.product_id,
            interface
                .interface_number
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string()),
            interface.usage_page,
            interface.usage,
            interface.release_number,
            interface.path_display()
        );
    }
    Ok(())
}

fn info(cli: &Cli) -> Result<()> {
    let (mut device, set) = open_device(cli)?;
    let (cid, mid) = device.check_psd()?;
    let (battery, charging) = device.get_power()?;
    let profile = device.read_profile()?;
    println!(
        "Режим:        {} (PID 0x{:04X})",
        set.connection.label(),
        set.command.product_id
    );
    println!("CID/MID:      0x{:02X}/0x{:02X}", cid, mid);
    println!("Прошивка:     0x{:04X}", set.command.release_number);
    println!(
        "Батарея:      {}%{}",
        battery,
        if charging {
            " (заряжается)"
        } else {
            ""
        }
    );
    println!(
        "DPI:          {}{}",
        profile
            .active_dpi()
            .map(|stage| format!("{}x{}", stage.x, stage.y))
            .unwrap_or_else(|| "n/a".to_string()),
        profile
            .active_dpi()
            .map(|_| format!(
                ", ступень {} из {}",
                profile.current_dpi_stage + 1,
                profile.dpi_stage_count
            ))
            .unwrap_or_default()
    );
    println!(
        "Polling:      {} Гц",
        profile
            .polling_hz
            .map(|v| v.to_string())
            .unwrap_or_else(|| "?".into())
    );
    println!("Debounce:     {} мс", profile.debounce);
    Ok(())
}

fn battery(cli: &Cli) -> Result<()> {
    let (mut device, set) = open_device(cli)?;
    let (battery, charging) = device.get_power()?;
    println!(
        "{}: {}%{}",
        set.connection.label(),
        battery,
        if charging {
            " (заряжается)"
        } else {
            ""
        }
    );
    Ok(())
}

fn dump_all(cli: &Cli, out: &Path) -> Result<()> {
    let (mut device, set) = open_device(cli)?;
    let dump = save_dump(&mut device, &set, out)?;
    println!(
        "Сохранено: {} ({} байт профиля)",
        out.display(),
        dump.profile.raw.len()
    );
    Ok(())
}

fn get(cli: &Cli, what: &GetCommands) -> Result<()> {
    let (mut device, set) = open_device(cli)?;
    let profile = device.read_profile()?;
    match what {
        GetCommands::Dpi => {
            println!("Активная ступень: {}", profile.current_dpi_stage + 1);
            for (index, stage) in profile.dpi_stages.iter().enumerate() {
                let active = if index == profile.current_dpi_stage as usize {
                    "*"
                } else {
                    " "
                };
                println!(
                    "{active} {:>2}: {:>6}x{:<6} mul={} raw={:02X?} chk={}",
                    index + 1,
                    stage.x,
                    stage.y,
                    stage.multiplier,
                    stage.raw,
                    stage.checksum_ok
                );
            }
        }
        GetCommands::Led => {
            println!("main: {:02X?}", profile.main_led);
            println!("logo: {:02X?}", profile.logo_led);
            println!("extra: {:02X?}", profile.led_extra);
        }
        GetCommands::Debounce => println!("{} мс", profile.debounce),
        GetCommands::Lod => println!("{}", profile.lod),
        GetCommands::Polling => println!(
            "{} Гц (код {})",
            profile
                .polling_hz
                .map(|v| v.to_string())
                .unwrap_or_else(|| "?".into()),
            profile.polling_code
        ),
        GetCommands::AngleSnap => println!("{}", profile.param_af),
        GetCommands::Ripple => println!("{}", profile.param_b1),
        GetCommands::Keys => {
            for (index, key) in profile.keys.iter().enumerate() {
                println!(
                    "{:>2}: code=0x{:08X} chk={}",
                    index + 1,
                    key.code,
                    key.checksum_ok
                );
            }
        }
        GetCommands::Profile => {
            println!("{}", serde_json::to_string_pretty(&profile)?);
        }
    }
    let _ = set;
    Ok(())
}

fn set(cli: &Cli, what: &SetCommands) -> Result<()> {
    match what {
        SetCommands::Dpi { stage, x, y, write } => {
            require_write(write)?;
            if *stage >= 8 {
                bail!("ступень DPI должна быть 0..7");
            }
            let (mut device, profile, _) = open_for_write(cli, write.backup_dir.clone())?;
            if let Some(old) = profile.dpi_stages.get(*stage).copied() {
                println!("Было:  {}x{} (raw {:02X?})", old.x, old.y, old.raw);
            }
            let new = device.set_dpi_stage(*stage, *x, *y)?;
            println!("Стало: {}x{} (raw {:02X?})", new.x, new.y, new.raw);
            println!("Запись и чтение обратно: OK");
        }
        SetCommands::Debounce { value, write } => {
            require_write(write)?;
            let (mut device, profile, _) = open_for_write(cli, write.backup_dir.clone())?;
            println!("Было:  {} мс", profile.debounce);
            device.set_debounce(*value)?;
            println!("Стало: {} мс, чтение обратно OK", value);
        }
        SetCommands::Led {
            mode,
            speed,
            brightness,
            write,
        } => {
            require_write(write)?;
            let (mut device, profile, _) = open_for_write(cli, write.backup_dir.clone())?;
            println!(
                "Было:  mode/speed/brightness = {:?}",
                profile.main_led_state()
            );
            device.set_led_state(*mode, *speed, *brightness)?;
            println!(
                "Стало: mode/speed/brightness = ({}, {}, {}), чтение обратно OK",
                mode, speed, brightness
            );
        }
        SetCommands::DpiColor {
            stage,
            r,
            g,
            b,
            write,
        } => {
            require_write(write)?;
            if *stage >= 8 {
                bail!("ступень DPI должна быть 0..7");
            }
            let (mut device, profile, _) = open_for_write(cli, write.backup_dir.clone())?;
            if let Some(color) = profile.color_for_stage(*stage) {
                println!("Было:  R={} G={} B={}", color.r, color.g, color.b);
            }
            device.set_dpi_color(*stage, *r, *g, *b)?;
            println!("Стало: R={} G={} B={}, чтение обратно OK", r, g, b);
        }
        SetCommands::Key {
            slot,
            code,
            action,
            write,
        } => {
            require_write(write)?;
            if *slot >= 16 {
                bail!("слот кнопки должен быть 0..15");
            }
            let code = match (code.as_ref(), action.as_ref()) {
                (Some(code), None) => *code,
                (None, Some(name)) => KeyAction::from_name(name.as_str())
                    .ok_or_else(|| anyhow!("неизвестное действие кнопки: {name}"))?
                    .hardware_code(),
                _ => bail!("укажи ровно одно: --code или --action"),
            };
            let (mut device, profile, _) = open_for_write(cli, write.backup_dir.clone())?;
            if let Some(old) = profile.keys.get(*slot) {
                println!("Было:  0x{:08X}", old.code);
            }
            device.set_key_raw(*slot, code)?;
            println!("Стало: 0x{:08X}, чтение обратно OK", code);
        }
        SetCommands::Lod { value, write } => {
            require_write(write)?;
            let (mut device, profile, _) = open_for_write(cli, write.backup_dir.clone())?;
            println!("Было:  LOD {}", profile.lod);
            device.set_lod(*value)?;
            println!("Стало: LOD {}, чтение обратно OK", value);
        }
        SetCommands::Polling { hz, write } => {
            require_write(write)?;
            let (mut device, profile, _) = open_for_write(cli, write.backup_dir.clone())?;
            println!(
                "Было:  {} Гц (код {})",
                profile
                    .polling_hz
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "?".into()),
                profile.polling_code
            );
            device.set_polling_hz(*hz)?;
            println!("Стало: {} Гц, чтение обратно OK", hz);
        }
        SetCommands::AngleSnap { value, write } => {
            require_write(write)?;
            if *value > 1 {
                bail!("angle snap: 0 или 1");
            }
            let (mut device, profile, _) = open_for_write(cli, write.backup_dir.clone())?;
            println!("Было:  angle snap {}", profile.param_af);
            device.set_angle_snap(*value != 0)?;
            println!("Стало: angle snap {}, чтение обратно OK", value);
        }
        SetCommands::Ripple { value, write } => {
            require_write(write)?;
            if *value > 1 {
                bail!("ripple: 0 или 1");
            }
            let (mut device, profile, _) = open_for_write(cli, write.backup_dir.clone())?;
            println!("Было:  ripple {}", profile.param_b1);
            device.set_ripple(*value != 0)?;
            println!("Стало: ripple {}, чтение обратно OK", value);
        }
        SetCommands::ProfileReset { write } => {
            require_write(write)?;
            let (mut device, _profile, _) = open_for_write(cli, write.backup_dir.clone())?;
            println!("Сброс текущего профиля...");
            device.reset_profile()?;
            let profile = device.read_profile()?;
            println!(
                "Профиль сброшен: {} ступеней DPI, polling {} Гц, debounce {} мс, LOD {}",
                profile.dpi_stage_count,
                profile
                    .polling_hz
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "?".into()),
                profile.debounce,
                profile.lod
            );
        }
    }
    Ok(())
}

fn parse_u32(text: &str) -> Result<u32, String> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).map_err(|err| err.to_string())
    } else {
        text.parse::<u32>().map_err(|err| err.to_string())
    }
}

fn require_write(options: &WriteOptions) -> Result<()> {
    if !options.allow_write {
        bail!("запись запрещена: добавь --allow-write");
    }
    Ok(())
}

fn open_for_write(
    cli: &Cli,
    backup_dir: Option<PathBuf>,
) -> Result<(BoxedDevice, Profile, PathBuf)> {
    let (mut device, device_set) = open_device(cli)?;
    let backup_dir = backup_dir.unwrap_or_else(|| PathBuf::from("."));
    let backup_path = backup_dir.join(format!("backup_{}.json", unix_now()?));
    let dump = save_dump(&mut device, &device_set, &backup_path)
        .with_context(|| format!("не удалось создать backup {}", backup_path.display()))?;
    println!("Backup: {}", backup_path.display());
    Ok((device, dump.profile, backup_path))
}

fn monitor(cli: &Cli, seconds: u64) -> Result<()> {
    let (mut device, set) = open_device(cli)?;
    println!(
        "monitor {} секунд, режим {}",
        seconds,
        set.connection.label()
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
    while std::time::Instant::now() < deadline {
        if let Some(frame) = device
            .transport_mut()
            .read_notify(Duration::from_millis(200))?
        {
            let bytes = frame.as_bytes();
            println!(
                "cmd=0x{:02X} status=0x{:02X} {}",
                frame.command(),
                frame.status(),
                bytes
                    .iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
    }
    Ok(())
}

fn restore_backup(cli: &Cli, backup: &Path, allow_write: bool) -> Result<()> {
    if !allow_write {
        bail!("restore-backup требует --allow-write");
    }
    if !backup.is_file() {
        bail!("backup не найден: {}", backup.display());
    }
    let dump: Dump = serde_json::from_slice(&fs::read(backup)?)
        .with_context(|| format!("не удалось разобрать backup {}", backup.display()))?;
    if dump.profile.dpi_stages.len() < 8 {
        bail!("в backup нет 8 ступеней DPI");
    }
    let (mut device, device_set) = open_device(cli)?;
    let pre_dir = backup.parent().unwrap_or_else(|| Path::new("."));
    let pre_path = pre_dir.join(format!("pre_restore_{}.json", unix_now()?));
    save_dump(&mut device, &device_set, &pre_path).with_context(|| {
        format!(
            "не удалось сохранить текущее состояние {}",
            pre_path.display()
        )
    })?;
    println!("Текущее состояние: {}", pre_path.display());

    let profile = &dump.profile;
    if matches!(profile.polling_code, 1 | 2 | 4 | 8) {
        device.set_polling_code(profile.polling_code)?;
        println!("Polling: код {}", profile.polling_code);
    }
    device.write_verified(
        aj199_protocol::addr::DPI_STAGE_COUNT,
        &aj199_protocol::pair(profile.dpi_stage_count),
    )?;
    device.write_verified(
        aj199_protocol::addr::CURRENT_DPI_STAGE,
        &aj199_protocol::pair(profile.current_dpi_stage),
    )?;
    for (stage, value) in profile.dpi_stages.iter().enumerate().take(8) {
        let restored = device.set_dpi_stage(stage, value.x, value.y)?;
        println!(
            "DPI {}: {}x{} -> {:02X?}",
            stage + 1,
            restored.x,
            restored.y,
            restored.raw
        );
    }
    for (stage, color) in profile.dpi_colors.iter().enumerate().take(8) {
        device.set_dpi_color(stage, color.r, color.g, color.b)?;
    }
    println!("Цвета DPI: восстановлены 8 ступеней");
    device.set_debounce(profile.debounce)?;
    device.set_lod(profile.lod)?;
    device.set_angle_snap(profile.param_af != 0)?;
    device.set_ripple(profile.param_b1 != 0)?;
    device.write_verified(aj199_protocol::addr::MAIN_LED, &profile.main_led[..6])?;
    println!(
        "LED: mode/speed/brightness = {:?}",
        profile.main_led_state()
    );
    for (slot, key) in profile.keys.iter().enumerate().take(16) {
        device.set_key_raw(slot, key.code)?;
    }
    println!("Кнопки: восстановлены 16 слотов");
    println!("Готово. Не восстановлены: логотип LED, 0xA0..0xA7, 0xAB..0xB7 и макросы.");
    Ok(())
}
fn open_device(cli: &Cli) -> Result<(BoxedDevice, DeviceSet)> {
    let timeout = Duration::from_millis(cli.timeout_ms);
    let retries = cli.retries.max(1);
    if let Some(kind) = cli.simulator {
        let (connection, pid, battery, charging) = match kind {
            SimulatorKind::Wired => (ConnectionKind::Wired, 0xFA7B, 95, true),
            SimulatorKind::Dongle => (ConnectionKind::Dongle, 0xFA7C, 85, false),
        };
        let sim =
            SimulatorTransport::from_hex(connection, pid, battery, charging, SIM_PROFILE_HEX)?;
        let set = sim
            .find_devices()?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("simulator device not found"))?;
        let transport: Box<dyn Transport> = Box::new(sim);
        let device = Aj199Device::open(transport, &set, timeout, retries)?;
        return Ok((device, set));
    }

    let hid = HidTransport::new()?;
    let sets = hid.find_devices()?;
    let set = sets
        .into_iter()
        .next()
        .ok_or(TransportError::DeviceNotFound)?;
    let transport: Box<dyn Transport> = Box::new(hid);
    let device = Aj199Device::open(transport, &set, timeout, retries)?;
    Ok((device, set))
}

fn save_dump(device: &mut BoxedDevice, set: &DeviceSet, out: &Path) -> Result<Dump> {
    let profile = device.read_profile()?;
    let dump = Dump {
        metadata: Metadata {
            generated_at_unix: unix_now()?,
            connection: set.connection.label().to_string(),
            vendor_id: set.command.vendor_id,
            pid: set.command.product_id,
            product: set.command.product.clone(),
            release_number: set.command.release_number,
            command_path: set.command.path_display(),
            notify_path: set.notify.path_display(),
        },
        raw_hex: profile
            .raw
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" "),
        profile,
    };
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(out, serde_json::to_vec_pretty(&dump)?)?;
    Ok(dump)
}

fn diff(first: &Path, second: &Path) -> Result<()> {
    let left: Dump = serde_json::from_slice(&fs::read(first)?)?;
    let right: Dump = serde_json::from_slice(&fs::read(second)?)?;
    if left.raw_hex == right.raw_hex {
        println!("Профили идентичны (0 различий)");
        return Ok(());
    }
    let left_value = serde_json::to_value(&left.profile)?;
    let right_value = serde_json::to_value(&right.profile)?;
    let mut differences = 0usize;
    if let (serde_json::Value::Object(a), serde_json::Value::Object(b)) = (left_value, right_value)
    {
        for key in a
            .keys()
            .chain(b.keys())
            .collect::<std::collections::BTreeSet<_>>()
        {
            if a.get(key) != b.get(key) {
                differences += 1;
                println!("DIFF {key}");
                println!(
                    "  {}: {}",
                    first.display(),
                    a.get(key).map(|v| v.to_string()).unwrap_or_default()
                );
                println!(
                    "  {}: {}",
                    second.display(),
                    b.get(key).map(|v| v.to_string()).unwrap_or_default()
                );
            }
        }
    }
    println!("Различий: {differences}");
    Ok(())
}

fn unix_now() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before UNIX_EPOCH")?
        .as_secs())
}
