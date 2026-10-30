//! Заряд мыши вне окна: иконка трея и системные уведомления в стиле темы.
//!
//! Окно при закрытии уничтожается, поэтому всё здесь работает без `WebView`:
//! иконка рисуется попиксельно, картинки уведомлений лежат на диске.

#[cfg(windows)]
use std::fmt::Write as _;
use std::path::Path;

pub const ICON_SIZE: u32 = 64;
const ICON_SIDE: u16 = 64;
const ICON_PIXELS: usize = 64;
const LOW_LEVEL: u8 = 20;
const CRITICAL_LEVEL: u8 = 10;
const FULL_LEVEL: u8 = 100;
/// Ступень заряда мыши 5%: сброс с запасом, чтобы не дребезжать на пороге.
const RESET_LEVEL: u8 = 25;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    pub battery: u8,
    pub charging: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alert {
    Low,
    Critical,
    Full,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AlertState {
    low_sent: bool,
    critical_sent: bool,
    full_sent: bool,
}

impl AlertState {
    /// Решает, нужно ли уведомление; каждое срабатывает один раз за цикл.
    pub const fn evaluate(&mut self, reading: Reading) -> Option<Alert> {
        let Reading { battery, charging } = reading;
        if charging {
            self.low_sent = false;
            self.critical_sent = false;
            if battery >= FULL_LEVEL && !self.full_sent {
                self.full_sent = true;
                return Some(Alert::Full);
            }
            return None;
        }
        self.full_sent = false;
        if battery == 0 {
            return None;
        }
        if battery >= RESET_LEVEL {
            self.low_sent = false;
            self.critical_sent = false;
            return None;
        }
        if battery <= CRITICAL_LEVEL && !self.critical_sent {
            self.critical_sent = true;
            self.low_sent = true;
            return Some(Alert::Critical);
        }
        if battery <= LOW_LEVEL && !self.low_sent {
            self.low_sent = true;
            return Some(Alert::Low);
        }
        None
    }
}

#[derive(Default)]
pub struct PowerState {
    pub theme: String,
    pub theme_icon: Option<Vec<u8>>,
    pub reading: Option<Reading>,
    pub alerts: AlertState,
    pub alerts_enabled: bool,
}

// ---------- Иконка трея ----------

type Rgb = (f32, f32, f32);

struct Palette {
    background: Rgb,
    line: Rgb,
    accent: Rgb,
    accent_2: Rgb,
    radius: f32,
    segments: u8,
}

const fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    (r as f32, g as f32, b as f32)
}

fn palette(theme: &str) -> Palette {
    match theme {
        "sunset" => Palette {
            background: rgb(0x1c, 0x0b, 0x18),
            line: rgb(0xff, 0xe2, 0xc8),
            accent: rgb(0xff, 0xc0, 0x6a),
            accent_2: rgb(0xff, 0x4f, 0x9a),
            radius: 16.0,
            segments: 0,
        },
        "graphite" => Palette {
            background: rgb(0x14, 0x14, 0x16),
            line: rgb(0xd6, 0xd8, 0xdc),
            accent: rgb(0xe8, 0xe9, 0xec),
            accent_2: rgb(0xa8, 0xac, 0xb4),
            radius: 6.0,
            segments: 5,
        },
        "phosphor" => Palette {
            background: rgb(0x03, 0x11, 0x0a),
            line: rgb(0x39, 0xff, 0x88),
            accent: rgb(0x39, 0xff, 0x88),
            accent_2: rgb(0x00, 0xc8, 0x78),
            radius: 8.0,
            segments: 4,
        },
        "fireflies" => Palette {
            background: rgb(0x0e, 0x1d, 0x15),
            line: rgb(0xf1, 0xe9, 0xc6),
            accent: rgb(0xff, 0xd3, 0x6b),
            accent_2: rgb(0x7f, 0xd8, 0x8f),
            radius: 30.0,
            segments: 0,
        },
        "aurora" => Palette {
            background: rgb(0x06, 0x0b, 0x18),
            line: rgb(0xe9, 0xf1, 0xff),
            accent: rgb(0x5e, 0xf2, 0xc2),
            accent_2: rgb(0xb4, 0x7c, 0xff),
            radius: 16.0,
            segments: 0,
        },
        _ => Palette {
            background: rgb(0x0b, 0x1a, 0x2c),
            line: rgb(0xcf, 0xef, 0xff),
            accent: rgb(0x6e, 0xe7, 0xff),
            accent_2: rgb(0x8b, 0x5c, 0xf6),
            radius: 16.0,
            segments: 0,
        },
    }
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    (
        (b.0 - a.0).mul_add(t, a.0),
        (b.1 - a.1).mul_add(t, a.1),
        (b.2 - a.2).mul_add(t, a.2),
    )
}

/// Покрытие пикселя скруглённым прямоугольником (0..1) со сглаживанием.
fn round_rect(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let cx = f32::midpoint(x0, x1);
    let cy = f32::midpoint(y0, y1);
    let dx = (px - cx).abs() - ((x1 - x0) / 2.0 - r);
    let dy = (py - cy).abs() - ((y1 - y0) / 2.0 - r);
    let outside = dx.max(0.0).hypot(dy.max(0.0));
    let distance = outside + dx.max(dy).min(0.0) - r;
    (0.5 - distance).clamp(0.0, 1.0)
}

fn in_triangle(p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
    let sign = |p1: (f32, f32), p2: (f32, f32), p3: (f32, f32)| {
        (p1.0 - p3.0).mul_add(p2.1 - p3.1, -(p2.0 - p3.0) * (p1.1 - p3.1))
    };
    let (d1, d2, d3) = (sign(p, a, b), sign(p, b, c), sign(p, c, a));
    !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
}

/// Цветовой канал уже в диапазоне 0..=255: приведение ограничено clamp.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
const fn channel(value: f32) -> u8 {
    value.clamp(0.0, 255.0) as u8
}

/// Батарея на фоне темы. Толстые линии: Windows ужимает иконку до 16–32 px.
pub fn draw_battery_icon(theme: &str, reading: Reading) -> Vec<u8> {
    let palette = palette(theme);
    let level = (f32::from(reading.battery) / 100.0).clamp(0.0, 1.0);
    let warning = if reading.charging {
        None
    } else if reading.battery <= CRITICAL_LEVEL {
        Some(rgb(0xff, 0x4d, 0x5e))
    } else if reading.battery <= LOW_LEVEL {
        Some(rgb(0xff, 0xb4, 0x3c))
    } else {
        None
    };
    let mut pixels = Vec::with_capacity(ICON_PIXELS * ICON_PIXELS * 4);
    let (inner_left, inner_right) = (13.0_f32, 47.0_f32);
    let fill_right = (inner_right - inner_left).mul_add(level.max(0.08), inner_left);
    for y in 0..ICON_SIDE {
        for x in 0..ICON_SIDE {
            let (px, py) = (f32::from(x) + 0.5, f32::from(y) + 0.5);
            let shape = round_rect(px, py, 0.0, 0.0, 64.0, 64.0, palette.radius);
            let mut color = palette.background;
            let body = round_rect(px, py, 5.0, 17.0, 55.0, 47.0, 7.0);
            let hollow = round_rect(px, py, 10.0, 22.0, 50.0, 42.0, 3.0);
            let outline = body * (1.0 - hollow);
            let cap = round_rect(px, py, 55.0, 26.0, 60.0, 38.0, 2.0);
            color = mix(color, palette.line, outline.max(cap));
            let mut fill = round_rect(px, py, inner_left, 25.0, fill_right, 39.0, 1.5);
            if palette.segments > 0 {
                let segments = f32::from(palette.segments);
                let cell = (inner_right - inner_left) / segments;
                let phase = ((px - inner_left) / cell).fract() * cell;
                let lit = ((px - inner_left) / cell).floor() + 1.0;
                let lit_level = (level * segments).ceil().max(1.0);
                if phase > cell - 2.0 || lit > lit_level {
                    fill = 0.0;
                } else {
                    fill = round_rect(px, py, inner_left, 25.0, inner_right, 39.0, 1.0);
                }
            }
            let gradient = ((px - inner_left) / (inner_right - inner_left)).clamp(0.0, 1.0);
            let fill_color =
                warning.unwrap_or_else(|| mix(palette.accent, palette.accent_2, gradient));
            color = mix(color, fill_color, fill);
            if reading.charging
                && (in_triangle((px, py), (35.0, 19.0), (22.0, 34.0), (31.0, 34.0))
                    || in_triangle((px, py), (29.0, 45.0), (42.0, 30.0), (33.0, 30.0)))
            {
                color = (255.0, 255.0, 255.0);
            }
            pixels.extend([
                channel(color.0),
                channel(color.1),
                channel(color.2),
                channel(shape * 255.0),
            ]);
        }
    }
    pixels
}

// ---------- Уведомления ----------

#[cfg_attr(not(windows), allow(dead_code))]
struct ToastText {
    title: String,
    body: String,
    status: &'static str,
    sound: &'static str,
}

struct ToastCopy {
    theme: &'static str,
    alert: Alert,
    title: &'static str,
    body: &'static str,
    status: &'static str,
    sound: &'static str,
}

#[rustfmt::skip]
const TOAST_COPY: [ToastCopy; 18] = [
    ToastCopy { theme: "sunset", alert: Alert::Low, title: "Солнце садится", body: "Мышь разряжается. Самое время найти кабель.", status: "вечереет", sound: "Notification.Default" },
    ToastCopy { theme: "sunset", alert: Alert::Critical, title: "Последние лучи", body: "Заряд почти на нуле, скоро стемнеет.", status: "сумерки", sound: "Notification.Reminder" },
    ToastCopy { theme: "sunset", alert: Alert::Full, title: "Рассвет", body: "Мышь полностью заряжена.", status: "новый день", sound: "Notification.IM" },
    ToastCopy { theme: "graphite", alert: Alert::Low, title: "Заряд снижен", body: "Рекомендуется подключить кабель.", status: "норма нарушена", sound: "Notification.Default" },
    ToastCopy { theme: "graphite", alert: Alert::Critical, title: "Заряд критический", body: "Подключите кабель.", status: "ниже допуска", sound: "Notification.Reminder" },
    ToastCopy { theme: "graphite", alert: Alert::Full, title: "Заряд завершён", body: "Кабель можно отключить.", status: "в допуске", sound: "Notification.Default" },
    ToastCopy { theme: "phosphor", alert: Alert::Low, title: "warn: battery low", body: "$ plug-in --cable", status: "low", sound: "Notification.SMS" },
    ToastCopy { theme: "phosphor", alert: Alert::Critical, title: "error: battery critical", body: "$ shutdown imminent", status: "critical", sound: "Notification.Reminder" },
    ToastCopy { theme: "phosphor", alert: Alert::Full, title: "ok: charge complete", body: "$ unplug --safe", status: "done", sound: "Notification.SMS" },
    ToastCopy { theme: "fireflies", alert: Alert::Low, title: "Светлячок тускнеет", body: "Заряд мыши подходит к концу.", status: "тускнеет", sound: "Notification.Mail" },
    ToastCopy { theme: "fireflies", alert: Alert::Critical, title: "Огонёк почти погас", body: "Подзаряди мышь, пока лес не уснул.", status: "гаснет", sound: "Notification.Reminder" },
    ToastCopy { theme: "fireflies", alert: Alert::Full, title: "Светлячок сияет", body: "Мышь полностью заряжена.", status: "сияет", sound: "Notification.Mail" },
    ToastCopy { theme: "aurora", alert: Alert::Low, title: "Сияние слабеет", body: "Заряд мыши подходит к концу.", status: "слабеет", sound: "Notification.Default" },
    ToastCopy { theme: "aurora", alert: Alert::Critical, title: "Небо темнеет", body: "Заряд почти на нуле.", status: "угасает", sound: "Notification.Reminder" },
    ToastCopy { theme: "aurora", alert: Alert::Full, title: "Небо в огнях", body: "Мышь полностью заряжена.", status: "в полную силу", sound: "Notification.IM" },
    ToastCopy { theme: "deep", alert: Alert::Low, title: "Кислород на исходе", body: "Заряд мыши снижается, пора всплывать к кабелю.", status: "погружение", sound: "Notification.Default" },
    ToastCopy { theme: "deep", alert: Alert::Critical, title: "Срочное всплытие", body: "Заряд почти на нуле.", status: "на дне", sound: "Notification.Reminder" },
    ToastCopy { theme: "deep", alert: Alert::Full, title: "Баллоны полны", body: "Мышь полностью заряжена.", status: "на поверхности", sound: "Notification.IM" },
];

fn toast_text(theme: &str, alert: Alert, battery: u8) -> ToastText {
    let copy = TOAST_COPY
        .iter()
        .find(|copy| copy.theme == theme && copy.alert == alert)
        .or_else(|| {
            TOAST_COPY
                .iter()
                .find(|copy| copy.theme == "deep" && copy.alert == alert)
        })
        .expect("в таблице есть тексты темы deep для каждого уведомления");
    ToastText {
        title: if theme == "phosphor" {
            format!("{} [{battery}%]", copy.title)
        } else {
            copy.title.to_string()
        },
        body: copy.body.to_string(),
        status: copy.status,
        sound: copy.sound,
    }
}

#[cfg(windows)]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(windows)]
fn file_uri(path: &Path) -> String {
    format!("file:///{}", path.to_string_lossy().replace('\\', "/"))
}

#[cfg(windows)]
fn toast_xml(theme: &str, alert: Alert, battery: u8, art_dir: &Path) -> String {
    let text = toast_text(theme, alert, battery);
    let hero = art_dir.join(format!(
        "{theme}-{}.png",
        if alert == Alert::Full { "full" } else { "low" }
    ));
    let logo = art_dir.join(format!("{theme}-logo.png"));
    let mut images = String::new();
    if hero.exists() {
        let _ = write!(
            images,
            r#"<image placement="hero" src="{}"/>"#,
            xml_escape(&file_uri(&hero))
        );
    }
    if logo.exists() {
        let crop = if theme == "fireflies" {
            r#" hint-crop="circle""#
        } else {
            ""
        };
        let _ = write!(
            images,
            r#"<image placement="appLogoOverride"{crop} src="{}"/>"#,
            xml_escape(&file_uri(&logo))
        );
    }
    let duration = if alert == Alert::Critical {
        "long"
    } else {
        "short"
    };
    format!(
        r#"<toast duration="{duration}"><visual><binding template="ToastGeneric">{images}<text>{title}</text><text>{body}</text><text placement="attribution">AJAZZ AJ199</text><progress title="Заряд" value="{value:.2}" valueStringOverride="{battery}%" status="{status}"/></binding></visual><audio src="ms-winsoundevent:{sound}"/></toast>"#,
        title = xml_escape(&text.title),
        body = xml_escape(&text.body),
        value = f32::from(battery) / 100.0,
        status = xml_escape(text.status),
        sound = text.sound,
    )
}

#[cfg(windows)]
const APP_ID: &str = "com.aj199.control";

/// Программа без установщика: Windows узнаёт имя и значок уведомлений из реестра.
#[cfg(windows)]
fn register_app(icon: Option<&std::path::PathBuf>) -> Result<(), String> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegSetKeyValueW};
    use windows::core::{HSTRING, PCWSTR};

    let key = HSTRING::from(format!(r"Software\Classes\AppUserModelId\{APP_ID}"));
    let set = |name: &str, value: &str| -> Result<(), String> {
        let name = HSTRING::from(name);
        let data: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let size = u32::try_from(data.len() * 2).map_err(|err| err.to_string())?;
        // SAFETY: строки живут до конца вызова, длина данных в байтах верна.
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                PCWSTR(name.as_ptr()),
                REG_SZ.0,
                Some(data.as_ptr().cast()),
                size,
            )
        };
        if status.is_ok() {
            Ok(())
        } else {
            Err(format!("реестр: {status:?}"))
        }
    };
    set("DisplayName", "AJ199 Control")?;
    if let Some(icon) = icon {
        set("IconUri", &icon.to_string_lossy())?;
    }
    Ok(())
}

/// На Linux уведомление отправляет `notify-send` с логотипом темы.
#[cfg(not(windows))]
pub fn show_toast(theme: &str, alert: Alert, battery: u8, art_dir: &Path) -> Result<(), String> {
    let text = toast_text(theme, alert, battery);
    let logo = art_dir.join(format!("{theme}-logo.png"));
    let urgency = if alert == Alert::Critical {
        "critical"
    } else {
        "normal"
    };
    let mut command = std::process::Command::new("notify-send");
    command.args(["--app-name=AJ199 Control", "--urgency", urgency]);
    if logo.exists() {
        command.arg("--icon").arg(logo);
    }
    command
        .arg(text.title)
        .arg(format!("{} ({battery}%)", text.body))
        .status()
        .map_err(|err| err.to_string())
        .map(|_| ())
}

#[cfg(windows)]
pub fn show_toast(theme: &str, alert: Alert, battery: u8, art_dir: &Path) -> Result<(), String> {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
    use windows::core::HSTRING;

    // SAFETY: повторная инициализация потока безопасна, результат не важен.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let logo = art_dir.join(format!("{theme}-logo.png"));
    register_app(logo.exists().then_some(&logo))?;
    let xml = toast_xml(theme, alert, battery, art_dir);
    let document = XmlDocument::new().map_err(|err| err.to_string())?;
    document
        .LoadXml(&HSTRING::from(xml))
        .map_err(|err| format!("XML уведомления: {err}"))?;
    let toast =
        ToastNotification::CreateToastNotification(&document).map_err(|err| err.to_string())?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))
        .and_then(|notifier| notifier.Show(&toast))
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(battery: u8, charging: bool) -> Reading {
        Reading { battery, charging }
    }

    #[test]
    fn alerts_fire_once_per_threshold() {
        let mut state = AlertState::default();
        assert_eq!(state.evaluate(reading(50, false)), None);
        assert_eq!(state.evaluate(reading(20, false)), Some(Alert::Low));
        assert_eq!(state.evaluate(reading(20, false)), None);
        assert_eq!(state.evaluate(reading(15, false)), None);
        assert_eq!(state.evaluate(reading(10, false)), Some(Alert::Critical));
        assert_eq!(state.evaluate(reading(5, false)), None);
    }

    #[test]
    fn first_reading_below_critical_skips_low() {
        let mut state = AlertState::default();
        assert_eq!(state.evaluate(reading(5, false)), Some(Alert::Critical));
        assert_eq!(state.evaluate(reading(15, false)), None);
    }

    #[test]
    fn jitter_at_threshold_does_not_repeat() {
        let mut state = AlertState::default();
        assert_eq!(state.evaluate(reading(20, false)), Some(Alert::Low));
        assert_eq!(state.evaluate(reading(21, false)), None);
        assert_eq!(state.evaluate(reading(20, false)), None);
        assert_eq!(state.evaluate(reading(25, false)), None);
        assert_eq!(state.evaluate(reading(20, false)), Some(Alert::Low));
    }

    #[test]
    fn full_fires_once_while_charging() {
        let mut state = AlertState::default();
        assert_eq!(state.evaluate(reading(95, true)), None);
        assert_eq!(state.evaluate(reading(100, true)), Some(Alert::Full));
        assert_eq!(state.evaluate(reading(100, true)), None);
        assert_eq!(state.evaluate(reading(100, false)), None);
        assert_eq!(state.evaluate(reading(100, true)), Some(Alert::Full));
    }

    #[test]
    fn charging_rearms_low_alert() {
        let mut state = AlertState::default();
        assert_eq!(state.evaluate(reading(20, false)), Some(Alert::Low));
        assert_eq!(state.evaluate(reading(20, true)), None);
        assert_eq!(state.evaluate(reading(20, false)), Some(Alert::Low));
    }

    #[test]
    fn icon_has_expected_size_and_transparent_corners() {
        for theme in [
            "deep",
            "sunset",
            "graphite",
            "phosphor",
            "fireflies",
            "aurora",
        ] {
            let pixels = draw_battery_icon(theme, reading(55, true));
            assert_eq!(pixels.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
            assert_eq!(pixels[3], 0, "{theme}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn toast_xml_is_escaped_and_has_progress() {
        let xml = toast_xml("phosphor", Alert::Low, 20, Path::new("nowhere"));
        assert!(xml.contains("warn: battery low [20%]"));
        assert!(xml.contains(r#"value="0.20""#));
        assert!(xml.contains("$ plug-in --cable"));
        assert!(!xml.contains("<image"));
    }
}
