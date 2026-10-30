//! Байтовый слой протокола AJAZZ AJ199.
//!
//! Здесь нет Windows HID API и знаний о том, кабель это или приёмник.
//! Все константы и форматы подтверждены кодом `OemDrv.exe` и записями
//! `startup_read_all_*` / `dpi_stage1_1600_to_800_*`.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

mod macro_codec;
pub use macro_codec::{
    SoftwareMacro, SoftwareMacroEvent, JMM_EVENTS_OFFSET, JMM_LEN, JMM_MAGIC,
    JMM_NAME_OFFSET, MACRO_MAX_EVENTS, MACRO_NAME_MAX_BYTES,
};

pub const VENDOR_ID: u16 = 0x25A7;
pub const PRODUCT_ID_WIRED: u16 = 0xFA7B;
pub const PRODUCT_ID_DONGLE: u16 = 0xFA7C;

pub const REPORT_LEN: usize = 17;
pub const FEATURE_REPORT_ID: u8 = 0x08;
pub const INPUT_REPORT_ID: u8 = 0x09;
pub const CHECKSUM_BASE: u8 = 0x55;
pub const PAYLOAD_LEN: usize = 14;
pub const EEPROM_CHUNK: usize = 10;

pub const PROFILE_START: u16 = 0x0000;
pub const PROFILE_LEN: usize = 0xB8;

pub mod addr {
    pub const POLLING: u16 = 0x00;
    pub const DPI_STAGE_COUNT: u16 = 0x02;
    pub const CURRENT_DPI_STAGE: u16 = 0x04;
    pub const LOD: u16 = 0x0A;
    pub const DPI_START: u16 = 0x0C;
    pub const DPI_COLOR_START: u16 = 0x2C;
    pub const MAIN_LED: u16 = 0x4C;
    pub const LOGO_LED: u16 = 0x54;
    pub const LED_EXTRA: u16 = 0x5C;
    pub const KEYS: u16 = 0x60;
    pub const MOUSE_PARAMS: u16 = 0xA0;
    pub const DEBOUNCE: u16 = 0xA9;
    pub const PARAM_AB: u16 = 0xAB;
    pub const SLEEP: u16 = 0xAD;
    pub const PARAM_AF: u16 = 0xAF;
    pub const PARAM_B1: u16 = 0xB1;
    pub const PARAM_B3: u16 = 0xB3;
    pub const PARAM_B5: u16 = 0xB5;
    pub const PARAM_B7: u16 = 0xB7;
}

/// Вид подключения.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionKind {
    Wired,
    Dongle,
    Unknown,
}

impl ConnectionKind {
    pub fn from_pid(pid: u16) -> Self {
        match pid {
            PRODUCT_ID_WIRED => Self::Wired,
            PRODUCT_ID_DONGLE => Self::Dongle,
            _ => Self::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Wired => "USB",
            Self::Dongle => "2.4G",
            Self::Unknown => "unknown",
        }
    }
}

/// Ошибки разбора и сборки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    WrongLength { expected: usize, actual: usize },
    WrongReportId(u8),
    ChecksumMismatch { expected: u8, actual: u8 },
    PayloadTooLarge { max: usize, actual: usize },
    UnsupportedCommand(u8),
    UnsupportedValue(&'static str),
    InvalidDpi(u16),
    InvalidAddress(u16),
    InvalidStage(usize),
}

impl core::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::WrongLength { expected, actual } => {
                write!(f, "wrong length: expected {expected}, got {actual}")
            }
            Self::WrongReportId(id) => write!(f, "unexpected report id 0x{id:02x}"),
            Self::ChecksumMismatch { expected, actual } => {
                write!(
                    f,
                    "checksum mismatch: expected 0x{expected:02x}, got 0x{actual:02x}"
                )
            }
            Self::PayloadTooLarge { max, actual } => {
                write!(f, "payload too large: max {max}, got {actual}")
            }
            Self::UnsupportedCommand(id) => write!(f, "unsupported command 0x{id:02x}"),
            Self::UnsupportedValue(value) => write!(f, "unsupported value: {value}"),
            Self::InvalidDpi(value) => write!(f, "invalid DPI: {value}"),
            Self::InvalidAddress(value) => write!(f, "invalid address: 0x{value:04x}"),
            Self::InvalidStage(value) => write!(f, "invalid DPI stage: {value}"),
        }
    }
}

impl std::error::Error for ProtocolError {}

/// Команды, подтверждённые записями.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Command {
    CheckPsd = 0x01,
    TellDrvStatus = 0x02,
    GetConnectStatus = 0x03,
    GetPower = 0x04,
    WriteEeprom = 0x07,
    ReadEeprom = 0x08,
    ProfileReset = 0x09,
    GetCurProfile = 0x0E,
    GetStatus = 0x0F,
    SetCurProfile = 0x10,
}

impl Command {
    pub fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0x01 => Self::CheckPsd,
            0x02 => Self::TellDrvStatus,
            0x03 => Self::GetConnectStatus,
            0x04 => Self::GetPower,
            0x07 => Self::WriteEeprom,
            0x08 => Self::ReadEeprom,
            0x09 => Self::ProfileReset,
            0x0E => Self::GetCurProfile,
            0x0F => Self::GetStatus,
            0x10 => Self::SetCurProfile,
            _ => return None,
        })
    }
}

/// Пара `[value, 0x55 - value]` для простых параметров.
pub fn pair(value: u8) -> [u8; 2] {
    [value, checksum(&[value])]
}

/// Основной LED: `[mode, chk, speed, chk, brightness, chk]`.
pub fn led_state(mode: u8, speed: u8, brightness: u8) -> [u8; 6] {
    [
        mode,
        checksum(&[mode]),
        speed,
        checksum(&[speed]),
        brightness,
        checksum(&[brightness]),
    ]
}

/// Цвет DPI/подсветки: `[R, G, B, checksum]`.
pub fn color(r: u8, g: u8, b: u8) -> [u8; 4] {
    [r, g, b, checksum(&[r, g, b])]
}

/// Известные действия кнопок и их hardware-коды.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyAction {
    Left,
    Right,
    Middle,
    Forward,
    Back,
    DpiUp,
    DpiDown,
    DpiLoop,
    Disable,
    Macro { id: u8, flag: u8 },
}

impl KeyAction {
    pub fn hardware_code(self) -> u32 {
        match self {
            Self::Left => 0x0101,
            Self::Right => 0x0201,
            Self::Middle => 0x0401,
            Self::Forward => 0x1001,
            Self::Back => 0x0801,
            Self::DpiUp => 0x0202,
            Self::DpiDown => 0x0302,
            Self::DpiLoop => 0x0102,
            Self::Disable => 0x000000,
            Self::Macro { id, flag } => ((flag as u32) << 16) | ((id as u32) << 8) | 0x06,
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        let normalized = name.to_ascii_lowercase().replace('_', "-");
        Some(match normalized.as_str() {
            "left" | "left-click" => Self::Left,
            "right" | "right-click" => Self::Right,
            "middle" | "middle-click" => Self::Middle,
            "forward" => Self::Forward,
            "back" => Self::Back,
            "dpi-up" | "dpi-plus" | "dpi+" => Self::DpiUp,
            "dpi-down" | "dpi-minus" | "dpi-" => Self::DpiDown,
            "dpi-loop" | "dpi-cycle" => Self::DpiLoop,
            "disable" | "off" => Self::Disable,
            _ => return None,
        })
    }
}

pub fn checksum(bytes: &[u8]) -> u8 {
    let sum: u32 = bytes.iter().map(|&b| b as u32).sum();
    (u32::from(CHECKSUM_BASE).wrapping_sub(sum) & 0xFF) as u8
}

/// Feature Report `0x08` (хост → мышь).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureFrame {
    bytes: [u8; REPORT_LEN],
}

impl FeatureFrame {
    pub fn new(command: Command, payload: &[u8]) -> Result<Self, ProtocolError> {
        if payload.len() > PAYLOAD_LEN {
            return Err(ProtocolError::PayloadTooLarge {
                max: PAYLOAD_LEN,
                actual: payload.len(),
            });
        }
        let mut bytes = [0u8; REPORT_LEN];
        bytes[0] = FEATURE_REPORT_ID;
        bytes[1] = command as u8;
        bytes[2..2 + payload.len()].copy_from_slice(payload);
        bytes[REPORT_LEN - 1] = checksum(&bytes[..REPORT_LEN - 1]);
        Ok(Self { bytes })
    }

    pub fn from_raw(bytes: [u8; REPORT_LEN]) -> Result<Self, ProtocolError> {
        if bytes[0] != FEATURE_REPORT_ID {
            return Err(ProtocolError::WrongReportId(bytes[0]));
        }
        Ok(Self { bytes })
    }

    pub fn as_bytes(&self) -> &[u8; REPORT_LEN] {
        &self.bytes
    }

    pub fn command(&self) -> u8 {
        self.bytes[1]
    }

    pub fn payload(&self) -> &[u8] {
        &self.bytes[2..REPORT_LEN - 1]
    }

    pub fn checksum(&self) -> u8 {
        self.bytes[REPORT_LEN - 1]
    }

    pub fn checksum_ok(&self) -> bool {
        checksum(&self.bytes[..REPORT_LEN - 1]) == self.checksum()
    }

    pub fn is_zero_checksum_probe(&self) -> bool {
        self.command() == Command::GetConnectStatus as u8 && self.checksum() == 0
    }

    pub fn check_psd(seed: [u8; 4]) -> Result<Self, ProtocolError> {
        Self::new(
            Command::CheckPsd,
            &[0x00, 0x00, 0x00, 0x08, seed[0], seed[1], seed[2], seed[3]],
        )
    }

    pub fn get_connect_status() -> Result<Self, ProtocolError> {
        Self::new(Command::GetConnectStatus, &[])
    }

    pub fn get_power() -> Result<Self, ProtocolError> {
        Self::new(Command::GetPower, &[])
    }

    pub fn get_cur_profile() -> Result<Self, ProtocolError> {
        Self::new(Command::GetCurProfile, &[])
    }

    pub fn read_eeprom(address: u16, len: u8) -> Result<Self, ProtocolError> {
        if len > EEPROM_CHUNK as u8 {
            return Err(ProtocolError::UnsupportedValue("EEPROM read chunk > 10"));
        }
        Self::new(
            Command::ReadEeprom,
            &[0x00, (address >> 8) as u8, address as u8, len],
        )
    }

    pub fn write_eeprom(address: u16, data: &[u8]) -> Result<Self, ProtocolError> {
        if data.len() > EEPROM_CHUNK {
            return Err(ProtocolError::UnsupportedValue("EEPROM write chunk > 10"));
        }
        let mut payload = [0u8; PAYLOAD_LEN];
        payload[0] = 0x00;
        payload[1] = (address >> 8) as u8;
        payload[2] = address as u8;
        payload[3] = data.len() as u8;
        payload[4..4 + data.len()].copy_from_slice(data);
        Self::new(Command::WriteEeprom, &payload[..4 + data.len()])
    }

    pub fn profile_reset() -> Result<Self, ProtocolError> {
        Self::new(Command::ProfileReset, &[])
    }

    pub fn set_cur_profile(profile: u8) -> Result<Self, ProtocolError> {
        Self::new(Command::SetCurProfile, &[0x00, 0x00, 0x00, 0x01, profile])
    }
}

/// Input Report `0x09` (мышь → хост).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputFrame {
    bytes: [u8; REPORT_LEN],
}

impl InputFrame {
    pub fn parse(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() != REPORT_LEN {
            return Err(ProtocolError::WrongLength {
                expected: REPORT_LEN,
                actual: bytes.len(),
            });
        }
        if bytes[0] != INPUT_REPORT_ID {
            return Err(ProtocolError::WrongReportId(bytes[0]));
        }
        let mut frame = [0u8; REPORT_LEN];
        frame.copy_from_slice(bytes);
        let expected = checksum(&frame[..REPORT_LEN - 1]);
        if expected != frame[REPORT_LEN - 1] {
            return Err(ProtocolError::ChecksumMismatch {
                expected,
                actual: frame[REPORT_LEN - 1],
            });
        }
        Ok(Self { bytes: frame })
    }

    pub fn as_bytes(&self) -> &[u8; REPORT_LEN] {
        &self.bytes
    }

    pub fn command(&self) -> u8 {
        self.bytes[1]
    }

    pub fn status(&self) -> u8 {
        self.bytes[2]
    }

    pub fn checksum_ok(&self) -> bool {
        checksum(&self.bytes[..REPORT_LEN - 1]) == self.bytes[REPORT_LEN - 1]
    }

    pub fn cid_mid(&self) -> Option<(u8, u8)> {
        (self.command() == Command::CheckPsd as u8).then(|| (self.bytes[10], self.bytes[11]))
    }

    pub fn power(&self) -> Option<(u8, bool)> {
        (self.command() == Command::GetPower as u8).then(|| (self.bytes[6], self.bytes[7] != 0))
    }

    pub fn eeprom_echo(&self) -> Option<(u16, u8)> {
        (self.command() == Command::ReadEeprom as u8
            || self.command() == Command::WriteEeprom as u8)
            .then(|| {
                (
                    ((self.bytes[3] as u16) << 8) | self.bytes[4] as u16,
                    self.bytes[5],
                )
            })
    }

    pub fn eeprom_data(&self, len: usize) -> Option<&[u8]> {
        (self.command() == Command::ReadEeprom as u8 && len <= 10 && 6 + len <= REPORT_LEN)
            .then(|| &self.bytes[6..6 + len])
    }
}

/// Одна ступень DPI в EEPROM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DpiStage {
    pub x: u16,
    pub y: u16,
    pub multiplier: u8,
    pub checksum_ok: bool,
    pub raw: [u8; 4],
}

impl DpiStage {
    pub fn decode(raw: [u8; 4]) -> Self {
        let hw_x = u16::from(raw[0]) | (u16::from(raw[2] & 0x0C) << 6);
        let hw_y = u16::from(raw[1]) | (u16::from(raw[2] & 0xC0) << 2);
        let multiplier = raw[2] & 0x03;
        let factor: u16 = if multiplier & 1 == 0 { 50 } else { 100 };
        Self {
            x: (hw_x + 1) * factor,
            y: (hw_y + 1) * factor,
            multiplier,
            checksum_ok: checksum(&raw[..3]) == raw[3],
            raw,
        }
    }

    pub fn encode(x: u16, y: u16) -> Result<Self, ProtocolError> {
        let (hw_x, mul_x) = encode_hw(x)?;
        let (hw_y, mul_y) = encode_hw(y)?;
        if hw_x > 0x3FF || hw_y > 0x3FF || mul_x != mul_y {
            return Err(ProtocolError::InvalidDpi(x.max(y)));
        }
        let mut raw = [0u8; 4];
        raw[0] = (hw_x & 0xFF) as u8;
        raw[1] = (hw_y & 0xFF) as u8;
        raw[2] = ((((hw_x >> 8) & 0x03) << 2) as u8)
            | ((((hw_y >> 8) & 0x03) << 6) as u8)
            | (mul_x & 0x03);
        raw[3] = checksum(&raw[..3]);
        Ok(Self::decode(raw))
    }
}

fn encode_hw(dpi: u16) -> Result<(u16, u8), ProtocolError> {
    if !(50..=26000).contains(&dpi) {
        return Err(ProtocolError::InvalidDpi(dpi));
    }
    let (hw, mul) = if dpi < 26100 {
        (dpi / 50 - 1, 0)
    } else {
        (dpi / 100 - 1, 1)
    };
    let decoded = (hw + 1) * if mul & 1 == 0 { 50 } else { 100 };
    if decoded != dpi {
        return Err(ProtocolError::InvalidDpi(dpi));
    }
    Ok((hw, mul))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DpiColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub checksum_ok: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeySlot {
    pub code: u32,
    pub checksum_ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub raw: Vec<u8>,
    pub polling_code: u8,
    pub polling_hz: Option<u16>,
    pub dpi_stage_count: u8,
    pub current_dpi_stage: u8,
    pub lod: u8,
    pub dpi_stages: Vec<DpiStage>,
    pub dpi_colors: Vec<DpiColor>,
    pub main_led: [u8; 8],
    pub logo_led: [u8; 8],
    pub led_extra: [u8; 4],
    pub keys: Vec<KeySlot>,
    pub mouse_params: [u8; 8],
    pub debounce: u8,
    pub param_ab: u8,
    pub sleep: u8,
    pub param_af: u8,
    pub param_b1: u8,
    pub param_b3: u8,
    pub param_b5: u8,
    pub param_b7: u8,
}

impl Profile {
    pub fn parse(raw: &[u8]) -> Result<Self, ProtocolError> {
        if raw.len() < PROFILE_LEN {
            return Err(ProtocolError::WrongLength {
                expected: PROFILE_LEN,
                actual: raw.len(),
            });
        }
        let raw = &raw[..PROFILE_LEN];
        let polling_code = raw[addr::POLLING as usize];
        let polling_hz = match polling_code {
            1 => Some(1000),
            2 => Some(500),
            4 => Some(250),
            8 => Some(125),
            _ => None,
        };
        let mut dpi_stages = Vec::with_capacity(8);
        for stage in 0..8 {
            let offset = addr::DPI_START as usize + stage * 4;
            let mut value = [0u8; 4];
            value.copy_from_slice(&raw[offset..offset + 4]);
            dpi_stages.push(DpiStage::decode(value));
        }
        let mut dpi_colors = Vec::with_capacity(8);
        for stage in 0..8 {
            let offset = addr::DPI_COLOR_START as usize + stage * 4;
            dpi_colors.push(DpiColor {
                r: raw[offset],
                g: raw[offset + 1],
                b: raw[offset + 2],
                checksum_ok: checksum(&raw[offset..offset + 3]) == raw[offset + 3],
            });
        }
        let mut keys = Vec::with_capacity(16);
        for slot in 0..16 {
            let offset = addr::KEYS as usize + slot * 4;
            let code = u32::from(raw[offset])
                | (u32::from(raw[offset + 1]) << 8)
                | (u32::from(raw[offset + 2]) << 16);
            keys.push(KeySlot {
                code,
                checksum_ok: checksum(&raw[offset..offset + 3]) == raw[offset + 3],
            });
        }
        let mut main_led = [0u8; 8];
        main_led.copy_from_slice(&raw[addr::MAIN_LED as usize..addr::MAIN_LED as usize + 8]);
        let mut logo_led = [0u8; 8];
        logo_led.copy_from_slice(&raw[addr::LOGO_LED as usize..addr::LOGO_LED as usize + 8]);
        let mut led_extra = [0u8; 4];
        led_extra.copy_from_slice(&raw[addr::LED_EXTRA as usize..addr::LED_EXTRA as usize + 4]);
        let mut mouse_params = [0u8; 8];
        mouse_params
            .copy_from_slice(&raw[addr::MOUSE_PARAMS as usize..addr::MOUSE_PARAMS as usize + 8]);
        Ok(Self {
            raw: raw.to_vec(),
            polling_code,
            polling_hz,
            dpi_stage_count: raw[addr::DPI_STAGE_COUNT as usize],
            current_dpi_stage: raw[addr::CURRENT_DPI_STAGE as usize],
            lod: raw[addr::LOD as usize],
            dpi_stages,
            dpi_colors,
            main_led,
            logo_led,
            led_extra,
            keys,
            mouse_params,
            debounce: raw[addr::DEBOUNCE as usize],
            param_ab: raw[addr::PARAM_AB as usize],
            sleep: raw[addr::SLEEP as usize],
            param_af: raw[addr::PARAM_AF as usize],
            param_b1: raw[addr::PARAM_B1 as usize],
            param_b3: raw[addr::PARAM_B3 as usize],
            param_b5: raw[addr::PARAM_B5 as usize],
            param_b7: raw[addr::PARAM_B7 as usize],
        })
    }

    pub fn main_led_state(&self) -> (u8, u8, u8) {
        (self.main_led[0], self.main_led[2], self.main_led[4])
    }

    pub fn color_for_stage(&self, stage: usize) -> Option<&DpiColor> {
        (stage < 8).then(|| &self.dpi_colors[stage])
    }

    pub fn active_dpi(&self) -> Option<&DpiStage> {
        let index = self.current_dpi_stage as usize;
        if index < self.dpi_stage_count as usize {
            self.dpi_stages.get(index)
        } else {
            None
        }
    }

    pub fn set_stage(&mut self, stage: usize, x: u16, y: u16) -> Result<[u8; 4], ProtocolError> {
        if stage >= 8 {
            return Err(ProtocolError::InvalidStage(stage));
        }
        let value = DpiStage::encode(x, y)?;
        let offset = addr::DPI_START as usize + stage * 4;
        self.raw[offset..offset + 4].copy_from_slice(&value.raw);
        self.dpi_stages[stage] = value;
        Ok(value.raw)
    }
}

/// Разбивает чтение EEPROM на порции не больше 10 байт.
pub fn eeprom_chunks(start: u16, len: usize) -> Vec<(u16, u8)> {
    let mut chunks = Vec::new();
    let mut offset = 0;
    while offset < len {
        let chunk = (len - offset).min(EEPROM_CHUNK);
        chunks.push((start + offset as u16, chunk as u8));
        offset += chunk;
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_checksum() {
        let frame = FeatureFrame::get_connect_status().unwrap();
        assert_eq!(frame.as_bytes()[16], 0x4A);
        assert!(frame.checksum_ok());
    }

    #[test]
    fn check_psd_payload() {
        let frame = FeatureFrame::check_psd([0x4A, 0x2E, 0x45, 0x35]).unwrap();
        assert_eq!(
            frame.as_bytes()[..10],
            [0x08, 0x01, 0x00, 0x00, 0x00, 0x08, 0x4A, 0x2E, 0x45, 0x35]
        );
        assert!(frame.checksum_ok());
    }

    #[test]
    fn read_eeprom_frame() {
        let frame = FeatureFrame::read_eeprom(0x0C, 4).unwrap();
        assert_eq!(frame.as_bytes()[..6], [0x08, 0x08, 0x00, 0x00, 0x0C, 0x04]);
    }

    #[test]
    fn key_actions() {
        assert_eq!(KeyAction::Left.hardware_code(), 0x0101);
        assert_eq!(KeyAction::DpiUp.hardware_code(), 0x0202);
        assert_eq!(KeyAction::from_name("DPI+"), Some(KeyAction::DpiUp));
        assert_eq!(
            KeyAction::Macro { id: 5, flag: 0xFE }.hardware_code(),
            0xFE0506
        );
    }

    #[test]
    fn dpi_800_and_26000() {
        let stage = DpiStage::encode(800, 800).unwrap();
        assert_eq!(stage.raw, [0x0F, 0x0F, 0x00, 0x37]);
        let stage = DpiStage::encode(26000, 26000).unwrap();
        assert_eq!(stage.raw, [0x07, 0x07, 0x88, 0xBF]);
    }

    #[test]
    fn decode_known_input() {
        let frame =
            InputFrame::parse(&hex("09 01 00 00 00 08 83 36 A8 C0 40 01 00 00 00 00 E1")).unwrap();
        assert_eq!(frame.command(), 0x01);
        assert_eq!(frame.cid_mid(), Some((0x40, 0x01)));
    }

    #[test]
    fn power_frame() {
        let frame =
            InputFrame::parse(&hex("09 04 00 00 00 02 55 00 00 00 00 00 00 00 00 00 F1")).unwrap();
        assert_eq!(frame.power(), Some((0x55, false)));
    }

    #[test]
    fn parse_recorded_profile() {
        let raw = hex(include_str!("../../../fixtures/profile_dongle.hex").trim());
        let profile = Profile::parse(&raw).unwrap();
        assert_eq!(profile.polling_hz, Some(1000));
        assert_eq!(profile.dpi_stage_count, 1);
        assert_eq!(profile.dpi_stages[0].x, 1600);
        assert_eq!(profile.dpi_stages[5].x, 26000);
        assert_eq!(profile.debounce, 8);
        assert_eq!(profile.active_dpi().unwrap().x, 1600);
    }

    fn hex(text: &str) -> Vec<u8> {
        text.split_whitespace()
            .map(|part| u8::from_str_radix(part, 16).unwrap())
            .collect()
    }
}
