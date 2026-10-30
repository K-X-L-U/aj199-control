//! Фоновый исполнитель команд мыши.
//!
//! Один поток владеет открытым донглом и выполняет задания по очереди,
//! поэтому команды Tauri не занимают главный поток. Профиль (0x00..0xB8)
//! читается из мыши один раз и дальше живёт в памяти: `Recorder` видит
//! каждую успешную запись EEPROM и переносит её байты в кэш, так что после
//! записи профиль не перечитывается целиком.

use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use aj199_device::{
    Aj199Device, DeviceSet, FeatureFrame, HidTransport, InputFrame, Profile, Transport,
    TransportError,
};

const TIMEOUT_MS: u64 = 2500;
const RETRIES: usize = 3;
const PROBE_TIMEOUT_MS: u64 = 300;
const CMD_WRITE_EEPROM: u8 = 0x07;
const CMD_PROFILE_RESET: u8 = 0x09;
const CMD_SET_CUR_PROFILE: u8 = 0x10;

/// HID-транспорт, который запоминает отправленные записи EEPROM.
pub struct Recorder {
    inner: HidTransport,
    writes: Vec<(u16, Vec<u8>)>,
    invalidate: bool,
}

impl Transport for Recorder {
    fn find_devices(&self) -> Result<Vec<DeviceSet>, TransportError> {
        self.inner.find_devices()
    }

    fn open(&mut self, device: &DeviceSet) -> Result<(), TransportError> {
        self.inner.open(device)
    }

    fn send_feature(&mut self, frame: &FeatureFrame) -> Result<(), TransportError> {
        let bytes = frame.as_bytes();
        match frame.command() {
            CMD_WRITE_EEPROM => {
                let address = u16::from_be_bytes([bytes[3], bytes[4]]);
                let len = (bytes[5] as usize).min(bytes.len() - 6);
                self.writes.push((address, bytes[6..6 + len].to_vec()));
            }
            CMD_PROFILE_RESET | CMD_SET_CUR_PROFILE => self.invalidate = true,
            _ => {}
        }
        self.inner.send_feature(frame)
    }

    fn read_notify(&mut self, timeout: Duration) -> Result<Option<InputFrame>, TransportError> {
        self.inner.read_notify(timeout)
    }

    fn close(&mut self) {
        self.inner.close();
    }
}

pub type Device = Aj199Device<Recorder>;

pub struct Session {
    pub device: Device,
    pub set: DeviceSet,
    raw: Option<Vec<u8>>,
    pub cid: u8,
    pub mid: u8,
    pub battery: u8,
    pub charging: bool,
    /// Второе подключение с коротким таймаутом: спящая мышь не отвечает,
    /// и обычные 7,5 с ожидания блокировали бы очередь команд.
    probe: Option<Aj199Device<HidTransport>>,
}

impl Session {
    fn open_once() -> Result<Self, String> {
        let transport = HidTransport::new().map_err(|err| err.to_string())?;
        let devices = transport.find_devices().map_err(|err| err.to_string())?;
        let set = devices
            .into_iter()
            .next()
            .ok_or_else(|| "AJAZZ AJ199 не найден".to_string())?;
        let recorder = Recorder {
            inner: transport,
            writes: Vec::new(),
            invalidate: false,
        };
        let device = Aj199Device::open(recorder, &set, Duration::from_millis(TIMEOUT_MS), RETRIES)
            .map_err(|err| err.to_string())?;
        Ok(Self {
            device,
            set,
            raw: None,
            cid: 0,
            mid: 0,
            battery: 0,
            charging: false,
            probe: None,
        })
    }

    fn open() -> Result<Self, String> {
        let mut last_error = String::new();
        for attempt in 0..3u64 {
            match Self::open_once() {
                Ok(session) => return Ok(session),
                Err(error) => {
                    last_error = error;
                    if attempt < 2 {
                        thread::sleep(Duration::from_millis(250 * (attempt + 1)));
                    }
                }
            }
        }
        Err(last_error)
    }

    /// Профиль из памяти; из мыши читается только если кэша нет.
    pub fn profile(&mut self) -> Result<Profile, String> {
        if self.raw.is_none() {
            let raw = self
                .device
                .read_eeprom(aj199_protocol::PROFILE_START, aj199_protocol::PROFILE_LEN)
                .map_err(|err| err.to_string())?;
            self.raw = Some(raw);
        }
        Profile::parse(self.raw.as_deref().unwrap_or_default()).map_err(|err| err.to_string())
    }

    /// Полная сверка с мышью: профиль, PSD и заряд.
    pub fn resync(&mut self) -> Result<Profile, String> {
        self.raw = None;
        let profile = self.profile()?;
        if let Ok((cid, mid)) = self.device.check_psd() {
            self.cid = cid;
            self.mid = mid;
        }
        self.refresh_power();
        Ok(profile)
    }

    fn refresh_power(&mut self) {
        if let Ok((battery, charging)) = self.device.get_power() {
            self.battery = battery;
            self.charging = charging;
        }
    }

    /// Быстрая проверка, что мышь отвечает; `None` — спит или недоступна.
    pub fn power_tick(&mut self) -> Option<(u8, bool)> {
        if self.probe.is_none() {
            let transport = HidTransport::new().ok()?;
            let probe = Aj199Device::open(
                transport,
                &self.set,
                Duration::from_millis(PROBE_TIMEOUT_MS),
                1,
            )
            .ok()?;
            self.probe = Some(probe);
        }
        let probe = self.probe.as_mut()?;
        // Сон определяем чтением памяти мыши: ответ на запрос заряда может
        // прийти от приёмника.
        probe
            .read_eeprom(aj199_protocol::addr::DPI_STAGE_COUNT, 1)
            .ok()?;
        let (battery, charging) = probe.get_power().ok()?;
        self.battery = battery;
        self.charging = charging;
        Some((battery, charging))
    }

    fn begin(&mut self) {
        let recorder = self.device.transport_mut();
        recorder.writes.clear();
        recorder.invalidate = false;
    }

    /// Переносит записанные байты в кэш профиля.
    fn commit(&mut self) {
        let recorder = self.device.transport_mut();
        let writes = std::mem::take(&mut recorder.writes);
        let invalidate = std::mem::replace(&mut recorder.invalidate, false);
        if invalidate {
            self.raw = None;
            return;
        }
        let Some(raw) = self.raw.as_mut() else {
            return;
        };
        for (address, data) in writes {
            for (offset, byte) in data.iter().enumerate() {
                if let Some(slot) = raw.get_mut(address as usize + offset) {
                    *slot = *byte;
                }
            }
        }
    }

    fn rollback(&mut self) {
        let recorder = self.device.transport_mut();
        recorder.writes.clear();
        recorder.invalidate = false;
        self.raw = None;
    }
}

/// Результат поиска USB-устройств; `Err` — поиск не удался, сессию не трогаем.
pub type Probe = Result<Option<DeviceSet>, ()>;

/// Поиск идёт без радиообмена, но занимает ~130 мс, поэтому при опросе
/// статуса выполняется вне потока мыши и не задерживает запись.
pub fn probe_connection() -> Probe {
    let transport = HidTransport::new().map_err(|_| ())?;
    Ok(transport
        .find_devices()
        .ok()
        .and_then(|devices| devices.into_iter().next()))
}

pub struct Worker {
    session: Option<Session>,
}

pub type Job = Box<dyn FnOnce(&mut Worker) + Send>;

fn is_timeout(error: &str) -> bool {
    error.contains("timeout")
}

impl Worker {
    pub fn session(&mut self) -> Result<&mut Session, String> {
        if self.session.is_none() {
            self.session = Some(Session::open()?);
        }
        Ok(self.session.as_mut().expect("session just opened"))
    }

    /// Лучшее подключение определяется только при открытии, поэтому при
    /// опросе сверяемся со списком USB: кабель вставили или вынули, донгл
    /// выдернули — закрываем сессию, следующая команда откроет нужную.
    pub fn sync_connection(&mut self, probe: Probe) {
        let Ok(best) = probe else {
            return;
        };
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let unchanged = best.is_some_and(|set| {
            set.connection == session.set.connection && set.command.path == session.set.command.path
        });
        if !unchanged && let Some(mut session) = self.session.take() {
            session.device.close();
        }
    }

    /// После ошибки: таймаут (мышь спит) оставляет донгл открытым,
    /// остальное (донгл выдернут, сменился режим) закрывает сессию.
    fn fail(&mut self, error: &str) {
        if is_timeout(error) {
            if let Some(session) = self.session.as_mut() {
                session.rollback();
            }
        } else if let Some(mut session) = self.session.take() {
            session.device.close();
        }
    }

    /// Чтение без записи.
    pub fn read<R>(
        &mut self,
        job: impl FnOnce(&mut Session) -> Result<R, String>,
    ) -> Result<R, String> {
        let result = job(self.session()?);
        if let Err(error) = &result {
            self.fail(error);
        }
        result
    }

    /// Запись: кэш обновляется только если вся операция прошла успешно.
    pub fn write<R>(
        &mut self,
        job: impl FnOnce(&mut Session, &Profile) -> Result<R, String>,
    ) -> Result<R, String> {
        let result = (|| -> Result<R, String> {
            let session = self.session()?;
            let profile = session.profile()?;
            session.begin();
            let value = job(session, &profile)?;
            session.commit();
            Ok(value)
        })();
        if let Err(error) = &result {
            self.fail(error);
        }
        result
    }

    /// Полная сверка с повторным подключением: для явной сверки с мышью.
    pub fn reconnect_resync(&mut self) -> Result<Profile, String> {
        self.sync_connection(probe_connection());
        match self.read(Session::resync) {
            Ok(profile) => Ok(profile),
            Err(error) if is_timeout(&error) => Err(error),
            Err(_) => {
                self.session = None;
                self.read(Session::resync)
            }
        }
    }
}

pub fn spawn() -> Sender<Job> {
    let (sender, receiver) = mpsc::channel::<Job>();
    thread::Builder::new()
        .name("aj199-device".to_string())
        .spawn(move || {
            let mut worker = Worker { session: None };
            while let Ok(job) = receiver.recv() {
                job(&mut worker);
            }
        })
        .expect("spawn device worker");
    sender
}
