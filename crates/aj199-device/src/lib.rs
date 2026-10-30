//! Транспорт, поиск интерфейсов, повторы и симулятор AJAZZ AJ199.

#![forbid(unsafe_code)]

use std::collections::VecDeque;
use std::ffi::CString;
use std::thread;
use std::time::{Duration, Instant};

use hidapi::{HidApi, HidDevice};

pub use aj199_protocol::{
    ConnectionKind, DpiStage, FeatureFrame, InputFrame, PRODUCT_ID_DONGLE, PRODUCT_ID_WIRED,
    PROFILE_LEN, Profile, ProtocolError, SoftwareMacro, SoftwareMacroEvent, VENDOR_ID,
};

const CMD_USAGE_PAGE: u16 = 0xFF02;
const CMD_USAGE: u16 = 2;
const NOTIFY_USAGE_PAGE: u16 = 0xFF01;
const NOTIFY_USAGE: u16 = 0;

/// Описание одного HID-интерфейса.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceInfo {
    pub path: CString,
    pub vendor_id: u16,
    pub product_id: u16,
    pub interface_number: Option<i32>,
    pub usage_page: u16,
    pub usage: u16,
    pub release_number: u16,
    pub product: Option<String>,
    pub manufacturer: Option<String>,
    pub connection: ConnectionKind,
}

impl InterfaceInfo {
    pub fn path_display(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

/// Логическое устройство: hCmd + hNotify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceSet {
    pub command: InterfaceInfo,
    pub notify: InterfaceInfo,
    pub connection: ConnectionKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    DeviceNotFound,
    OpenFailed(String),
    IoFailed(String),
    Timeout { command: u8 },
    NotOpened,
    VerificationFailed,
    Protocol(ProtocolError),
    Simulator(String),
}

impl core::fmt::Display for TransportError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DeviceNotFound => write!(f, "device not found"),
            Self::OpenFailed(reason) => write!(f, "open failed: {reason}"),
            Self::IoFailed(reason) => write!(f, "i/o failed: {reason}"),
            Self::Timeout { command } => write!(f, "timeout waiting for command 0x{command:02x}"),
            Self::NotOpened => write!(f, "transport is not opened"),
            Self::VerificationFailed => write!(f, "read-back verification failed"),
            Self::Protocol(err) => write!(f, "protocol error: {err}"),
            Self::Simulator(reason) => write!(f, "simulator: {reason}"),
        }
    }
}

impl std::error::Error for TransportError {}

impl From<ProtocolError> for TransportError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

impl From<hidapi::HidError> for TransportError {
    fn from(value: hidapi::HidError) -> Self {
        Self::IoFailed(value.to_string())
    }
}

pub trait Transport {
    fn find_devices(&self) -> Result<Vec<DeviceSet>, TransportError>;
    fn open(&mut self, device: &DeviceSet) -> Result<(), TransportError>;
    fn send_feature(&mut self, frame: &FeatureFrame) -> Result<(), TransportError>;
    fn read_notify(&mut self, timeout: Duration) -> Result<Option<InputFrame>, TransportError>;
    fn close(&mut self);
}

impl<T: Transport + ?Sized> Transport for Box<T> {
    fn find_devices(&self) -> Result<Vec<DeviceSet>, TransportError> {
        (**self).find_devices()
    }

    fn open(&mut self, device: &DeviceSet) -> Result<(), TransportError> {
        (**self).open(device)
    }

    fn send_feature(&mut self, frame: &FeatureFrame) -> Result<(), TransportError> {
        (**self).send_feature(frame)
    }

    fn read_notify(&mut self, timeout: Duration) -> Result<Option<InputFrame>, TransportError> {
        (**self).read_notify(timeout)
    }

    fn close(&mut self) {
        (**self).close()
    }
}

/// Реальный HID-транспорт через hidapi (Windows native, на Linux — hidraw).
pub struct HidTransport {
    api: HidApi,
    command: Option<HidDevice>,
    notify: Option<HidDevice>,
    connection: ConnectionKind,
}

impl HidTransport {
    pub fn new() -> Result<Self, TransportError> {
        let api = HidApi::new().map_err(|err| TransportError::OpenFailed(err.to_string()))?;
        Ok(Self {
            api,
            command: None,
            notify: None,
            connection: ConnectionKind::Unknown,
        })
    }

    pub fn list_interfaces(&self) -> Result<Vec<InterfaceInfo>, TransportError> {
        let mut result = Vec::new();
        for info in self.api.device_list() {
            if info.vendor_id() != VENDOR_ID {
                continue;
            }
            let pid = info.product_id();
            if pid != PRODUCT_ID_WIRED && pid != PRODUCT_ID_DONGLE {
                continue;
            }
            result.push(InterfaceInfo {
                path: info.path().to_owned(),
                vendor_id: info.vendor_id(),
                product_id: pid,
                interface_number: Some(info.interface_number()),
                usage_page: info.usage_page(),
                usage: info.usage(),
                release_number: info.release_number(),
                product: info.product_string().map(str::to_owned),
                manufacturer: info.manufacturer_string().map(str::to_owned),
                connection: ConnectionKind::from_pid(pid),
            });
        }
        result.sort_by(|a, b| {
            a.product_id
                .cmp(&b.product_id)
                .then(a.usage_page.cmp(&b.usage_page))
                .then(a.usage.cmp(&b.usage))
        });
        Ok(result)
    }

    pub fn connection(&self) -> ConnectionKind {
        self.connection
    }
}

impl Transport for HidTransport {
    fn find_devices(&self) -> Result<Vec<DeviceSet>, TransportError> {
        let interfaces = self.list_interfaces()?;
        let mut devices = Vec::new();
        for pid in [PRODUCT_ID_WIRED, PRODUCT_ID_DONGLE] {
            let command = interfaces
                .iter()
                .find(|i| {
                    i.product_id == pid && i.usage_page == CMD_USAGE_PAGE && i.usage == CMD_USAGE
                })
                .cloned();
            let notify = interfaces
                .iter()
                .find(|i| {
                    i.product_id == pid
                        && i.usage_page == NOTIFY_USAGE_PAGE
                        && i.usage == NOTIFY_USAGE
                })
                .cloned();
            if let (Some(command), Some(notify)) = (command, notify) {
                devices.push(DeviceSet {
                    connection: ConnectionKind::from_pid(pid),
                    command,
                    notify,
                });
            }
        }
        if devices.is_empty() {
            return Err(TransportError::DeviceNotFound);
        }
        // Кабель имеет приоритет.
        devices.sort_by_key(|d| match d.connection {
            ConnectionKind::Wired => 0,
            ConnectionKind::Dongle => 1,
            ConnectionKind::Unknown => 2,
        });
        Ok(devices)
    }

    fn open(&mut self, device: &DeviceSet) -> Result<(), TransportError> {
        self.close();
        let command = self
            .api
            .open_path(&device.command.path)
            .map_err(|err| TransportError::OpenFailed(err.to_string()))?;
        let notify = self
            .api
            .open_path(&device.notify.path)
            .map_err(|err| TransportError::OpenFailed(err.to_string()))?;
        self.command = Some(command);
        self.notify = Some(notify);
        self.connection = device.connection;
        Ok(())
    }

    fn send_feature(&mut self, frame: &FeatureFrame) -> Result<(), TransportError> {
        let device = self.command.as_ref().ok_or(TransportError::NotOpened)?;
        device.send_feature_report(frame.as_bytes())?;
        Ok(())
    }

    fn read_notify(&mut self, timeout: Duration) -> Result<Option<InputFrame>, TransportError> {
        let device = self.notify.as_ref().ok_or(TransportError::NotOpened)?;
        let mut buffer = [0u8; 64];
        let timeout_ms = timeout.as_millis().min(i32::MAX as u128) as i32;
        let n = device.read_timeout(&mut buffer, timeout_ms)?;
        if n < aj199_protocol::REPORT_LEN {
            return Ok(None);
        }
        // На Linux один hidraw-узел отдаёт отчёты всех коллекций интерфейса,
        // чужие кадры (клавиатура и т. п.) просто пропускаем.
        Ok(InputFrame::parse(&buffer[..aj199_protocol::REPORT_LEN]).ok())
    }

    fn close(&mut self) {
        self.command = None;
        self.notify = None;
        self.connection = ConnectionKind::Unknown;
    }
}

/// Высокоуровневое устройство поверх любого транспорта.
pub struct Aj199Device<T: Transport> {
    transport: T,
    connection: ConnectionKind,
    timeout: Duration,
    retries: usize,
}

impl<T: Transport> Aj199Device<T> {
    pub fn open(
        mut transport: T,
        device: &DeviceSet,
        timeout: Duration,
        retries: usize,
    ) -> Result<Self, TransportError> {
        transport.open(device)?;
        Ok(Self {
            transport,
            connection: device.connection,
            timeout,
            retries: retries.max(1),
        })
    }

    pub fn connection(&self) -> ConnectionKind {
        self.connection
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    pub fn close(&mut self) {
        self.transport.close();
    }

    fn request_frame(&mut self, frame: FeatureFrame) -> Result<InputFrame, TransportError> {
        let command = frame.command();
        self.request_frame_with(frame, move |input| input.command() == command)
    }

    fn request_frame_with<F>(
        &mut self,
        frame: FeatureFrame,
        validate: F,
    ) -> Result<InputFrame, TransportError>
    where
        F: Fn(&InputFrame) -> bool,
    {
        for _ in 0..self.retries {
            self.transport.send_feature(&frame)?;
            let deadline = Instant::now() + self.timeout;
            loop {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                let remaining = deadline - now;
                let wait = remaining.min(Duration::from_millis(200));
                if let Some(input) = self.transport.read_notify(wait)?
                    && input.command() == frame.command()
                    && validate(&input)
                {
                    return Ok(input);
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
        Err(TransportError::Timeout {
            command: frame.command(),
        })
    }

    pub fn check_psd(&mut self) -> Result<(u8, u8), TransportError> {
        let seed = [
            rand::random::<u8>() % 100 + 1,
            rand::random::<u8>() % 100 + 1,
            rand::random::<u8>() % 100 + 1,
            rand::random::<u8>() % 100 + 1,
        ];
        let response = self.request_frame(FeatureFrame::check_psd(seed)?)?;
        response
            .cid_mid()
            .ok_or(TransportError::Protocol(ProtocolError::UnsupportedValue(
                "CheckPsd response",
            )))
    }

    pub fn get_connect_status(&mut self) -> Result<bool, TransportError> {
        let response = self.request_frame(FeatureFrame::get_connect_status()?)?;
        Ok(response.status() == 0)
    }

    pub fn reset_profile(&mut self) -> Result<(), TransportError> {
        let response = self.request_frame(FeatureFrame::profile_reset()?)?;
        if response.status() != 0 {
            return Err(TransportError::IoFailed(format!(
                "ProfileReset status {}",
                response.status()
            )));
        }
        Ok(())
    }

    pub fn get_power(&mut self) -> Result<(u8, bool), TransportError> {
        let response = self.request_frame(FeatureFrame::get_power()?)?;
        response
            .power()
            .ok_or(TransportError::Protocol(ProtocolError::UnsupportedValue(
                "GetPower response",
            )))
    }

    pub fn read_eeprom(&mut self, address: u16, len: usize) -> Result<Vec<u8>, TransportError> {
        let mut output = Vec::with_capacity(len);
        for (chunk_address, chunk_len) in aj199_protocol::eeprom_chunks(address, len) {
            let response = self.request_frame_with(
                FeatureFrame::read_eeprom(chunk_address, chunk_len)?,
                move |input| {
                    input.status() == 0 && input.eeprom_echo() == Some((chunk_address, chunk_len))
                },
            )?;
            if response.status() != 0 {
                return Err(TransportError::IoFailed(format!(
                    "ReadEEPROM status {} at 0x{chunk_address:04x}",
                    response.status()
                )));
            }
            if response.eeprom_echo() != Some((chunk_address, chunk_len)) {
                return Err(TransportError::IoFailed(format!(
                    "ReadEEPROM response does not echo 0x{chunk_address:04x}/{chunk_len}: {:?} (raw {:02X?})",
                    response.eeprom_echo(),
                    response.as_bytes()
                )));
            }
            let data = response
                .eeprom_data(chunk_len as usize)
                .ok_or(TransportError::IoFailed(
                    "ReadEEPROM response has no data".to_string(),
                ))?;
            output.extend_from_slice(data);
            thread::sleep(Duration::from_millis(5));
        }
        Ok(output)
    }

    pub fn write_eeprom(&mut self, address: u16, data: &[u8]) -> Result<(), TransportError> {
        for (offset, chunk) in data.chunks(aj199_protocol::EEPROM_CHUNK).enumerate() {
            let chunk_address = address + (offset * aj199_protocol::EEPROM_CHUNK) as u16;
            let response = self.request_frame_with(
                FeatureFrame::write_eeprom(chunk_address, chunk)?,
                move |input| {
                    input.status() == 0
                        && input.eeprom_echo() == Some((chunk_address, chunk.len() as u8))
                },
            )?;
            if response.status() != 0 {
                return Err(TransportError::IoFailed(format!(
                    "WriteEEPROM status {} at 0x{chunk_address:04x}",
                    response.status()
                )));
            }
            if response.eeprom_echo() != Some((chunk_address, chunk.len() as u8)) {
                return Err(TransportError::IoFailed(
                    "WriteEEPROM response does not echo address".to_string(),
                ));
            }
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    pub fn read_profile(&mut self) -> Result<Profile, TransportError> {
        let raw = self.read_eeprom(aj199_protocol::PROFILE_START, aj199_protocol::PROFILE_LEN)?;
        Ok(Profile::parse(&raw)?)
    }

    pub fn write_verified(&mut self, address: u16, data: &[u8]) -> Result<(), TransportError> {
        self.write_eeprom(address, data)?;
        thread::sleep(Duration::from_millis(20));
        let readback = self.read_eeprom(address, data.len())?;
        if readback != data {
            return Err(TransportError::VerificationFailed);
        }
        Ok(())
    }

    pub fn set_dpi_stage(
        &mut self,
        stage: usize,
        x: u16,
        y: u16,
    ) -> Result<DpiStage, TransportError> {
        if stage >= 8 {
            return Err(ProtocolError::InvalidStage(stage).into());
        }
        let value = DpiStage::encode(x, y)?;
        let address = aj199_protocol::addr::DPI_START + (stage * 4) as u16;
        self.write_verified(address, &value.raw)?;
        Ok(value)
    }

    pub fn set_current_dpi_stage(&mut self, stage: u8) -> Result<(), TransportError> {
        if stage >= 8 {
            return Err(ProtocolError::InvalidStage(stage as usize).into());
        }
        self.write_verified(
            aj199_protocol::addr::CURRENT_DPI_STAGE,
            &aj199_protocol::pair(stage),
        )
    }

    pub fn set_dpi_color(
        &mut self,
        stage: usize,
        r: u8,
        g: u8,
        b: u8,
    ) -> Result<(), TransportError> {
        if stage >= 8 {
            return Err(ProtocolError::InvalidStage(stage).into());
        }
        let address = aj199_protocol::addr::DPI_COLOR_START + (stage * 4) as u16;
        self.write_verified(address, &aj199_protocol::color(r, g, b))
    }

    pub fn set_polling_code(&mut self, code: u8) -> Result<(), TransportError> {
        if !matches!(code, 1 | 2 | 4 | 8) {
            return Err(ProtocolError::UnsupportedValue("polling code").into());
        }
        self.write_verified(aj199_protocol::addr::POLLING, &aj199_protocol::pair(code))
    }

    pub fn set_polling_hz(&mut self, hz: u16) -> Result<(), TransportError> {
        let code = match hz {
            1000 => 1,
            500 => 2,
            250 => 4,
            125 => 8,
            _ => return Err(ProtocolError::UnsupportedValue("polling rate").into()),
        };
        self.set_polling_code(code)
    }

    pub fn set_debounce(&mut self, value: u8) -> Result<(), TransportError> {
        if value > 20 {
            return Err(ProtocolError::UnsupportedValue("debounce").into());
        }
        self.write_verified(aj199_protocol::addr::DEBOUNCE, &aj199_protocol::pair(value))
    }

    pub fn set_lod(&mut self, value: u8) -> Result<(), TransportError> {
        if value > 3 {
            return Err(ProtocolError::UnsupportedValue("LOD").into());
        }
        self.write_verified(aj199_protocol::addr::LOD, &aj199_protocol::pair(value))
    }

    pub fn set_angle_snap(&mut self, enabled: bool) -> Result<(), TransportError> {
        self.write_verified(
            aj199_protocol::addr::PARAM_AF,
            &aj199_protocol::pair(u8::from(enabled)),
        )
    }

    pub fn set_ripple(&mut self, enabled: bool) -> Result<(), TransportError> {
        self.write_verified(
            aj199_protocol::addr::PARAM_B1,
            &aj199_protocol::pair(u8::from(enabled)),
        )
    }

    pub fn set_led_state(
        &mut self,
        mode: u8,
        speed: u8,
        brightness: u8,
    ) -> Result<(), TransportError> {
        if mode > 7 || brightness > 3 {
            return Err(ProtocolError::UnsupportedValue("LED mode/brightness").into());
        }
        self.write_verified(
            aj199_protocol::addr::MAIN_LED,
            &aj199_protocol::led_state(mode, speed, brightness),
        )
    }

    pub fn apply_known_profile(&mut self, profile: &Profile) -> Result<(), TransportError> {
        if matches!(profile.polling_code, 1 | 2 | 4 | 8) {
            self.set_polling_code(profile.polling_code)?;
        }
        self.write_verified(
            aj199_protocol::addr::DPI_STAGE_COUNT,
            &aj199_protocol::pair(profile.dpi_stage_count),
        )?;
        self.write_verified(
            aj199_protocol::addr::CURRENT_DPI_STAGE,
            &aj199_protocol::pair(profile.current_dpi_stage),
        )?;
        for (stage, value) in profile.dpi_stages.iter().enumerate().take(8) {
            self.set_dpi_stage(stage, value.x, value.y)?;
        }
        for (stage, color) in profile.dpi_colors.iter().enumerate().take(8) {
            self.set_dpi_color(stage, color.r, color.g, color.b)?;
        }
        self.set_debounce(profile.debounce)?;
        self.set_lod(profile.lod)?;
        self.set_angle_snap(profile.param_af != 0)?;
        self.set_ripple(profile.param_b1 != 0)?;
        self.write_verified(aj199_protocol::addr::MAIN_LED, &profile.main_led[..6])?;
        for (slot, key) in profile.keys.iter().enumerate().take(16) {
            self.set_key_raw(slot, key.code)?;
        }
        Ok(())
    }

    pub fn set_macro_name(&mut self, index: usize, name: &str) -> Result<(), TransportError> {
        if index >= 16 {
            return Err(TransportError::Simulator("macro index".to_string()));
        }
        let units: Vec<u16> = name.encode_utf16().collect();
        let byte_len = units.len() * 2;
        if byte_len > 30 {
            return Err(TransportError::Simulator("macro name too long".to_string()));
        }
        let mut data = [0u8; 31];
        data[0] = byte_len as u8;
        for (position, unit) in units.iter().enumerate() {
            let offset = 1 + position * 2;
            data[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
        }
        let base = 0x0300u16 + (index as u16) * 0x0180;
        self.write_verified(base, &data)
    }

    pub fn assign_macro(
        &mut self,
        macro_index: u8,
        slot: usize,
        flag: u8,
    ) -> Result<(), TransportError> {
        if macro_index >= 16 || slot >= 16 {
            return Err(TransportError::Simulator("macro assignment".to_string()));
        }
        let code = ((flag as u32) << 16) | ((macro_index as u32) << 8) | 0x06;
        self.set_key_raw(slot, code)
    }

    pub fn read_macro(&mut self, index: usize) -> Result<SoftwareMacro, TransportError> {
        if index >= 16 {
            return Err(TransportError::Simulator("macro index".to_string()));
        }
        let name = self.read_macro_name(index)?.unwrap_or_default();
        let base = 0x0300u16 + (index as u16) * 0x0180;
        let count = self.read_eeprom(base + 0x1F, 1)?[0] as usize;
        if count > aj199_protocol::MACRO_MAX_EVENTS {
            return Err(ProtocolError::UnsupportedValue("macro event count").into());
        }
        if count == 0 {
            return Ok(SoftwareMacro { name, events: Vec::new() });
        }
        let raw = self.read_eeprom(base + 0x20, count * 5)?;
        let events: Vec<[u8; 5]> = raw
            .chunks_exact(5)
            .map(|chunk| [chunk[0], chunk[1], chunk[2], chunk[3], chunk[4]])
            .collect();
        SoftwareMacro::from_hardware_events(name, &events).map_err(Into::into)
    }

    pub fn write_macro(&mut self, index: usize, macro_data: &SoftwareMacro) -> Result<(), TransportError> {
        if index >= 16 {
            return Err(TransportError::Simulator("macro index".to_string()));
        }
        let data = macro_data.encode_hardware_slot()?;
        let base = 0x0300u16 + (index as u16) * 0x0180;
        self.write_verified(base, &data)
    }

    pub fn read_macro_name(&mut self, index: usize) -> Result<Option<String>, TransportError> {
        if index >= 16 {
            return Ok(None);
        }
        let base = 0x0300u16 + (index as u16) * 0x0180;
        let length = self.read_eeprom(base, 1)?;
        let len = length[0] as usize;
        if len == 0 || len > 30 {
            return Ok(None);
        }
        let data = self.read_eeprom(base, 1 + len)?;
        let body = &data[1..1 + len];
        if body.len() % 2 != 0 {
            return Ok(None);
        }
        let units: Vec<u16> = body
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let name = String::from_utf16_lossy(&units)
            .trim_matches('\0')
            .to_string();
        Ok((!name.is_empty()).then_some(name))
    }

    pub fn set_key_raw(&mut self, slot: usize, code: u32) -> Result<(), TransportError> {
        if slot >= 16 || code > 0xFFFFFF {
            return Err(ProtocolError::UnsupportedValue("key slot/code").into());
        }
        let raw = [
            (code & 0xFF) as u8,
            ((code >> 8) & 0xFF) as u8,
            ((code >> 16) & 0xFF) as u8,
            aj199_protocol::checksum(&[
                (code & 0xFF) as u8,
                ((code >> 8) & 0xFF) as u8,
                ((code >> 16) & 0xFF) as u8,
            ]),
        ];
        let address = aj199_protocol::addr::KEYS + (slot * 4) as u16;
        self.write_verified(address, &raw)
    }
}

/// Симулятор на основе записанного профиля.
pub struct SimulatorTransport {
    connection: ConnectionKind,
    pid: u16,
    memory: Vec<u8>,
    initial_memory: Vec<u8>,
    battery: u8,
    charging: bool,
    cid: u8,
    mid: u8,
    queue: VecDeque<InputFrame>,
    opened: bool,
}

impl SimulatorTransport {
    pub fn new(
        connection: ConnectionKind,
        pid: u16,
        battery: u8,
        charging: bool,
        memory: Vec<u8>,
    ) -> Result<Self, TransportError> {
        if memory.len() < PROFILE_LEN {
            return Err(TransportError::Simulator("profile too short".to_string()));
        }
        let mut memory = memory;
        if memory.len() < 0x2000 {
            memory.resize(0x2000, 0);
        }
        Ok(Self {
            connection,
            pid,
            initial_memory: memory.clone(),
            memory,
            battery,
            charging,
            cid: 0x40,
            mid: 0x01,
            queue: VecDeque::new(),
            opened: false,
        })
    }

    pub fn from_hex(
        connection: ConnectionKind,
        pid: u16,
        battery: u8,
        charging: bool,
        hex: &str,
    ) -> Result<Self, TransportError> {
        let memory = hex
            .split_whitespace()
            .map(|part| {
                u8::from_str_radix(part, 16)
                    .map_err(|err| TransportError::Simulator(err.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(connection, pid, battery, charging, memory)
    }

    fn response(&self, command: u8, body: &[u8]) -> InputFrame {
        let mut report = [0u8; aj199_protocol::REPORT_LEN];
        report[0] = aj199_protocol::INPUT_REPORT_ID;
        report[1] = command;
        let n = body.len().min(14);
        report[2..2 + n].copy_from_slice(&body[..n]);
        report[16] = aj199_protocol::checksum(&report[..16]);
        InputFrame::parse(&report).expect("simulator response must be valid")
    }

    fn handle(&mut self, frame: &FeatureFrame) -> Result<InputFrame, TransportError> {
        let command = frame.command();
        let payload = frame.payload();
        match command {
            0x01 => Ok(self.response(0x01, &[0, 0, 0, 0x08, 0, 0, 0, 0, self.cid, self.mid])),
            0x03 => Ok(self.response(0x03, &[0, 0, 0, 1, 1])),
            0x04 => Ok(self.response(0x04, &[0, 0, 0, 2, self.battery, u8::from(self.charging)])),
            0x07 => {
                let address = (u16::from(payload[1]) << 8) | u16::from(payload[2]);
                let len = payload[3] as usize;
                let data = &payload[4..4 + len.min(10)];
                self.memory[address as usize..address as usize + data.len()].copy_from_slice(data);
                let mut body = Vec::new();
                body.extend_from_slice(&[0, payload[1], payload[2], payload[3]]);
                body.extend_from_slice(data);
                Ok(self.response(0x07, &body))
            }
            0x08 => {
                let address = (u16::from(payload[1]) << 8) | u16::from(payload[2]);
                let len = payload[3] as usize;
                let end = address as usize + len;
                if end > self.memory.len() {
                    return Err(TransportError::Simulator("read out of range".to_string()));
                }
                let data = self.memory[address as usize..end].to_vec();
                let mut body = Vec::new();
                body.extend_from_slice(&[0, payload[1], payload[2], payload[3]]);
                body.extend_from_slice(&data);
                Ok(self.response(0x08, &body))
            }
            0x09 => {
                self.memory.clone_from(&self.initial_memory);
                Ok(self.response(0x09, &[0, 0, 0, 0]))
            }
            0x0E => Ok(self.response(0x0E, &[0, 0, 0, 1, 0])),
            0x10 => Ok(self.response(0x10, &[0, 0, 0, 1, payload.get(4).copied().unwrap_or(0)])),
            other => Err(TransportError::Simulator(format!(
                "no response for command 0x{other:02x}"
            ))),
        }
    }
}

impl Transport for SimulatorTransport {
    fn find_devices(&self) -> Result<Vec<DeviceSet>, TransportError> {
        let make = |suffix: &str, usage_page: u16, usage: u16| InterfaceInfo {
            path: CString::new(format!("sim://{suffix}")).expect("static path"),
            vendor_id: VENDOR_ID,
            product_id: self.pid,
            interface_number: Some(1),
            usage_page,
            usage,
            release_number: 0,
            product: Some("AJAZZ AJ199 simulator".to_string()),
            manufacturer: Some("simulator".to_string()),
            connection: self.connection,
        };
        Ok(vec![DeviceSet {
            connection: self.connection,
            command: make("cmd", CMD_USAGE_PAGE, CMD_USAGE),
            notify: make("notify", NOTIFY_USAGE_PAGE, NOTIFY_USAGE),
        }])
    }

    fn open(&mut self, _device: &DeviceSet) -> Result<(), TransportError> {
        self.opened = true;
        self.queue.clear();
        Ok(())
    }

    fn send_feature(&mut self, frame: &FeatureFrame) -> Result<(), TransportError> {
        if !self.opened {
            return Err(TransportError::NotOpened);
        }
        let response = self.handle(frame)?;
        self.queue.push_back(response);
        Ok(())
    }

    fn read_notify(&mut self, _timeout: Duration) -> Result<Option<InputFrame>, TransportError> {
        if !self.opened {
            return Err(TransportError::NotOpened);
        }
        Ok(self.queue.pop_front())
    }

    fn close(&mut self) {
        self.opened = false;
        self.queue.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_hex() -> &'static str {
        include_str!("../../../fixtures/profile_dongle.hex")
    }

    #[test]
    fn simulator_read_dongle() {
        let transport =
            SimulatorTransport::from_hex(ConnectionKind::Dongle, 0xFA7C, 85, false, profile_hex())
                .unwrap();
        let devices = transport.find_devices().unwrap();
        let mut device =
            Aj199Device::open(transport, &devices[0], Duration::from_millis(50), 1).unwrap();
        let profile = device.read_profile().unwrap();
        assert_eq!(profile.polling_hz, Some(1000));
        assert_eq!(profile.active_dpi().unwrap().x, 1600);
        assert_eq!(device.get_power().unwrap(), (85, false));
    }

    #[test]
    fn simulator_read_wired_and_set_dpi() {
        let transport =
            SimulatorTransport::from_hex(ConnectionKind::Wired, 0xFA7B, 95, true, profile_hex())
                .unwrap();
        let devices = transport.find_devices().unwrap();
        let mut device =
            Aj199Device::open(transport, &devices[0], Duration::from_millis(50), 1).unwrap();
        assert_eq!(device.get_power().unwrap(), (95, true));
        let updated = device.set_dpi_stage(0, 800, 800).unwrap();
        assert_eq!(updated.raw, [0x0F, 0x0F, 0x00, 0x37]);
        let profile = device.read_profile().unwrap();
        assert_eq!(profile.dpi_stages[0].x, 800);
    }

    #[test]
    fn simulator_setters() {
        let transport =
            SimulatorTransport::from_hex(ConnectionKind::Dongle, 0xFA7C, 85, false, profile_hex())
                .unwrap();
        let devices = transport.find_devices().unwrap();
        let mut device =
            Aj199Device::open(transport, &devices[0], Duration::from_millis(50), 1).unwrap();
        device.set_polling_hz(500).unwrap();
        device.set_debounce(4).unwrap();
        device.set_lod(2).unwrap();
        device.set_angle_snap(true).unwrap();
        device.set_ripple(true).unwrap();
        device.set_led_state(2, 128, 3).unwrap();
        device.set_dpi_color(0, 255, 0, 0).unwrap();
        device.set_key_raw(5, 0xFE0506).unwrap();
        let profile = device.read_profile().unwrap();
        assert_eq!(profile.polling_hz, Some(500));
        assert_eq!(profile.debounce, 4);
        assert_eq!(profile.lod, 2);
        assert_eq!(profile.param_af, 1);
        assert_eq!(profile.param_b1, 1);
        assert_eq!(profile.main_led_state(), (2, 128, 3));
        assert_eq!(
            profile.color_for_stage(0).map(|c| (c.r, c.g, c.b)),
            Some((255, 0, 0))
        );
        assert_eq!(profile.keys[5].code, 0xFE0506);
    }

    #[test]
    fn simulator_profile_reset() {
        let transport =
            SimulatorTransport::from_hex(ConnectionKind::Dongle, 0xFA7C, 85, false, profile_hex())
                .unwrap();
        let devices = transport.find_devices().unwrap();
        let mut device =
            Aj199Device::open(transport, &devices[0], Duration::from_millis(50), 1).unwrap();
        device.set_key_raw(5, 0x0202).unwrap();
        assert_eq!(device.read_profile().unwrap().keys[5].code, 0x0202);
        device.reset_profile().unwrap();
        assert_eq!(device.read_profile().unwrap().keys[5].code, 0xFE0506);
    }

    #[test]
    fn simulator_current_stage() {
        let transport =
            SimulatorTransport::from_hex(ConnectionKind::Dongle, 0xFA7C, 85, false, profile_hex())
                .unwrap();
        let devices = transport.find_devices().unwrap();
        let mut device =
            Aj199Device::open(transport, &devices[0], Duration::from_millis(50), 1).unwrap();
        device.set_current_dpi_stage(1).unwrap();
        assert_eq!(device.read_profile().unwrap().current_dpi_stage, 1);
    }

    #[test]
    fn simulator_macro_name_and_assignment() {
        let transport =
            SimulatorTransport::from_hex(ConnectionKind::Dongle, 0xFA7C, 85, false, profile_hex())
                .unwrap();
        let devices = transport.find_devices().unwrap();
        let mut device =
            Aj199Device::open(transport, &devices[0], Duration::from_millis(50), 1).unwrap();
        device.set_macro_name(7, "test").unwrap();
        assert_eq!(device.read_macro_name(7).unwrap().as_deref(), Some("test"));
        device.assign_macro(7, 5, 0xFE).unwrap();
        assert_eq!(device.read_profile().unwrap().keys[5].code, 0xFE0706);
    }

    #[test]
    fn simulator_macro_read_write() {
        let transport =
            SimulatorTransport::from_hex(ConnectionKind::Dongle, 0xFA7C, 85, false, profile_hex())
                .unwrap();
        let devices = transport.find_devices().unwrap();
        let mut device =
            Aj199Device::open(transport, &devices[0], Duration::from_millis(50), 1).unwrap();
        let macro_data = SoftwareMacro {
            name: "hello".to_string(),
            events: vec![
                SoftwareMacroEvent { key: 0x41, state: 1, delay_ms: 12 },
                SoftwareMacroEvent { key: 0x41, state: 0, delay_ms: 34 },
            ],
        };
        device.write_macro(3, &macro_data).unwrap();
        assert_eq!(device.read_macro(3).unwrap(), macro_data);
        assert_eq!(device.read_macro_name(3).unwrap().as_deref(), Some("hello"));
    }

    #[test]
    fn simulator_check_psd() {
        let transport =
            SimulatorTransport::from_hex(ConnectionKind::Dongle, 0xFA7C, 85, false, profile_hex())
                .unwrap();
        let devices = transport.find_devices().unwrap();
        let mut device =
            Aj199Device::open(transport, &devices[0], Duration::from_millis(50), 1).unwrap();
        assert_eq!(device.check_psd().unwrap(), (0x40, 0x01));
    }
}
