//! Кодек макросов `.jmm` и аппаратного формата AJAZZ.

use serde::{Deserialize, Serialize};

use crate::ProtocolError;

pub const JMM_LEN: usize = 0x342;
pub const JMM_MAGIC: u32 = 0xFACB_99A0;
pub const JMM_NAME_OFFSET: usize = 0x14;
pub const JMM_EVENTS_OFFSET: usize = 0x6A;
pub const MACRO_NAME_MAX_BYTES: usize = 30;
pub const MACRO_MAX_EVENTS: usize = 70;

/// Пары `[hid, software_key]` из OemDrv.exe (0x5B2EB8).
const HID_KEY_TABLE: [u8; 0x68 * 2] = [
    0x29, 0x1b, 0x3a, 0x70, 0x3b, 0x71, 0x3c, 0x72, 0x3d, 0x73, 0x3e, 0x74,
    0x3f, 0x75, 0x40, 0x76, 0x41, 0x77, 0x42, 0x78, 0x43, 0x79, 0x44, 0x7a,
    0x45, 0x7b, 0x04, 0x41, 0x05, 0x42, 0x06, 0x43, 0x07, 0x44, 0x08, 0x45,
    0x09, 0x46, 0x0a, 0x47, 0x0b, 0x48, 0x0c, 0x49, 0x0d, 0x4a, 0x0e, 0x4b,
    0x0f, 0x4c, 0x10, 0x4d, 0x11, 0x4e, 0x12, 0x4f, 0x13, 0x50, 0x14, 0x51,
    0x15, 0x52, 0x16, 0x53, 0x17, 0x54, 0x18, 0x55, 0x19, 0x56, 0x1a, 0x57,
    0x1b, 0x58, 0x1c, 0x59, 0x1d, 0x5a, 0x1e, 0x31, 0x1f, 0x32, 0x20, 0x33,
    0x21, 0x34, 0x22, 0x35, 0x23, 0x36, 0x24, 0x37, 0x25, 0x38, 0x26, 0x39,
    0x27, 0x30, 0x2b, 0x09, 0x39, 0x14, 0x28, 0x0d, 0x2a, 0x08, 0x2c, 0x20,
    0x2f, 0xdb, 0x30, 0xdd, 0x31, 0xdc, 0x33, 0xba, 0x34, 0xde, 0x36, 0xbc,
    0x37, 0xbe, 0x38, 0xbf, 0x35, 0xc0, 0x2e, 0xbb, 0x2d, 0xbd, 0x46, 0x2c,
    0x47, 0x91, 0x48, 0x13, 0x49, 0x2d, 0x4a, 0x24, 0x4b, 0x21, 0x4c, 0x2e,
    0x4d, 0x23, 0x4e, 0x22, 0x52, 0x26, 0x51, 0x28, 0x50, 0x25, 0x4f, 0x27,
    0x59, 0x61, 0x5a, 0x62, 0x5b, 0x63, 0x5c, 0x64, 0x5d, 0x65, 0x5e, 0x66,
    0x5f, 0x67, 0x60, 0x68, 0x61, 0x69, 0x62, 0x60, 0x57, 0x6b, 0x56, 0x6d,
    0x55, 0x6a, 0x54, 0x6f, 0x53, 0x90, 0x58, 0x0d, 0x63, 0x6e, 0xe0, 0xa2,
    0xe1, 0xa0, 0xe2, 0xa4, 0xe3, 0x5b, 0xe4, 0xa3, 0xe5, 0xa1, 0xe6, 0xa5,
    0xe7, 0x5c, 0x65, 0x5d,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareMacroEvent {
    pub key: u16,
    pub state: u16,
    pub delay_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareMacro {
    pub name: String,
    pub events: Vec<SoftwareMacroEvent>,
}

fn rotate_left2(data: &[u8]) -> Vec<u8> {
    data.iter()
        .map(|value| ((value << 2) & 0xFF) | (value >> 6))
        .collect()
}

fn rotate_right2(data: &[u8]) -> Vec<u8> {
    data.iter()
        .map(|value| (value >> 2) | ((value << 6) & 0xFF))
        .collect()
}

impl SoftwareMacro {
    pub fn parse_jmm(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() != JMM_LEN {
            return Err(ProtocolError::WrongLength {
                expected: JMM_LEN,
                actual: bytes.len(),
            });
        }
        let data = rotate_left2(bytes);
        let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        if magic != JMM_MAGIC {
            return Err(ProtocolError::UnsupportedValue(".jmm magic"));
        }

        let mut name_units = Vec::new();
        let mut offset = JMM_NAME_OFFSET;
        while offset + 1 < JMM_EVENTS_OFFSET {
            let unit = u16::from_le_bytes([data[offset], data[offset + 1]]);
            if unit == 0 {
                break;
            }
            name_units.push(unit);
            offset += 2;
        }
        let name = String::from_utf16_lossy(&name_units);

        let mut events = Vec::new();
        let mut offset = JMM_EVENTS_OFFSET;
        while offset + 8 <= data.len() {
            let key = u16::from_le_bytes([data[offset], data[offset + 1]]);
            if key == 0 {
                break;
            }
            let state = u16::from_le_bytes([data[offset + 2], data[offset + 3]]);
            let delay_ms = u32::from_le_bytes([
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ]);
            events.push(SoftwareMacroEvent { key, state, delay_ms });
            offset += 8;
        }
        Ok(Self { name, events })
    }

    pub fn to_jmm(&self) -> Result<Vec<u8>, ProtocolError> {
        let mut data = vec![0u8; JMM_LEN];
        data[0..4].copy_from_slice(&JMM_MAGIC.to_le_bytes());
        data[0x0C..0x10].copy_from_slice(&1u32.to_le_bytes());
        data[0x10..0x14].copy_from_slice(&(self.events.len() as u32).to_le_bytes());

        let units: Vec<u16> = self.name.encode_utf16().collect();
        for (index, unit) in units.iter().enumerate() {
            let offset = JMM_NAME_OFFSET + index * 2;
            if offset + 2 > JMM_EVENTS_OFFSET {
                return Err(ProtocolError::UnsupportedValue("macro name too long"));
            }
            data[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
        }

        let max_events = (JMM_LEN - JMM_EVENTS_OFFSET - 8) / 8;
        if self.events.len() > max_events {
            return Err(ProtocolError::UnsupportedValue("too many macro events"));
        }
        for (index, event) in self.events.iter().enumerate() {
            let offset = JMM_EVENTS_OFFSET + index * 8;
            data[offset..offset + 2].copy_from_slice(&event.key.to_le_bytes());
            data[offset + 2..offset + 4].copy_from_slice(&event.state.to_le_bytes());
            data[offset + 4..offset + 8].copy_from_slice(&event.delay_ms.to_le_bytes());
        }
        Ok(rotate_right2(&data))
    }

    pub fn to_hardware_events(&self) -> Result<Vec<[u8; 5]>, ProtocolError> {
        if self.events.len() > MACRO_MAX_EVENTS {
            return Err(ProtocolError::UnsupportedValue("too many macro events"));
        }
        let mut result = Vec::with_capacity(self.events.len());
        for event in &self.events {
            let delay = event.delay_ms.min(u16::MAX as u32) as u16;
            let delay_bytes = delay.to_be_bytes();
            if event.state & 2 != 0 {
                result.push([
                    5,
                    event.key as u8,
                    (event.key >> 8) as u8,
                    delay_bytes[0],
                    delay_bytes[1],
                ]);
                continue;
            }
            let (hid, kind) = software_to_hid(event.key)?;
            let flags = ((((event.state & 1) + 1) * 0x40) as u8) | kind;
            result.push([flags, hid, 0, delay_bytes[0], delay_bytes[1]]);
        }
        Ok(result)
    }

    pub fn from_hardware_events(name: String, events: &[[u8; 5]]) -> Result<Self, ProtocolError> {
        let mut result = Vec::with_capacity(events.len());
        for event in events {
            let flags = event[0];
            let delay_ms = u32::from(event[3]) << 8 | u32::from(event[4]);
            if flags == 5 {
                result.push(SoftwareMacroEvent {
                    key: u16::from(event[1]) | (u16::from(event[2]) << 8),
                    state: 2,
                    delay_ms,
                });
                continue;
            }
            let state = u16::from(flags & 0x80 != 0);
            let kind = flags & 0x07;
            let key = hid_to_software(event[1], kind)
                .ok_or(ProtocolError::UnsupportedValue("unknown hardware macro key"))?;
            result.push(SoftwareMacroEvent { key, state, delay_ms });
        }
        Ok(Self { name, events: result })
    }

    pub fn encode_hardware_slot(&self) -> Result<Vec<u8>, ProtocolError> {
        let name_units: Vec<u16> = self.name.encode_utf16().collect();
        let name_bytes = name_units.len() * 2;
        if name_bytes > MACRO_NAME_MAX_BYTES {
            return Err(ProtocolError::UnsupportedValue("macro name too long"));
        }
        let hardware = self.to_hardware_events()?;
        // Пишем весь слот, чтобы старые события за новым count не остались.
        let mut data = vec![0u8; 0x180];
        data[0] = name_bytes as u8;
        let mut offset = 1;
        for unit in &name_units {
            data[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
            offset += 2;
        }
        data[31] = hardware.len() as u8;
        offset = 32;
        for event in &hardware {
            data[offset..offset + 5].copy_from_slice(event);
            offset += 5;
        }
        Ok(data)
    }
}

fn software_to_hid(key: u16) -> Result<(u8, u8), ProtocolError> {
    let special = match key {
        0x10 | 0xA0 => Some((2, 0)),
        0x11 | 0xA2 => Some((1, 0)),
        0x12 | 0xA4 => Some((4, 0)),
        0x5B | 0x5C => Some((8, 0)),
        0xA1 => Some((0x20, 0)),
        0xA3 => Some((0x10, 0)),
        0xA5 => Some((0x40, 0)),
        0xF0 => Some((1, 4)),
        0xF1 => Some((2, 4)),
        0xF2 => Some((4, 4)),
        0xF3 => Some((0x10, 4)),
        0xF4 => Some((8, 4)),
        _ => None,
    };
    if let Some(value) = special {
        return Ok(value);
    }
    let key = key as u8;
    for pair in HID_KEY_TABLE.chunks_exact(2) {
        if pair[1] == key {
            return Ok((pair[0], 1));
        }
    }
    Err(ProtocolError::UnsupportedValue("unknown macro key"))
}

fn hid_to_software(hid: u8, kind: u8) -> Option<u16> {
    match kind {
        0 => Some(match hid {
            1 => 0xA2,
            2 => 0xA0,
            4 => 0xA4,
            8 => 0x5B,
            0x10 => 0xA3,
            0x20 => 0xA1,
            0x40 => 0xA5,
            _ => return None,
        }),
        4 => Some(match hid {
            1 => 0xF0,
            2 => 0xF1,
            4 => 0xF2,
            8 => 0xF4,
            0x10 => 0xF3,
            _ => return None,
        }),
        1 => {
            for pair in HID_KEY_TABLE.chunks_exact(2) {
                if pair[0] == hid {
                    return Some(u16::from(pair[1]));
                }
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_abc_jmm() {
        let bytes = include_bytes!("../../../fixtures/abc.jmm");
        let macro_file = SoftwareMacro::parse_jmm(bytes).unwrap();
        assert_eq!(macro_file.name, "abc");
        assert_eq!(macro_file.events.len(), 2);
        assert_eq!(macro_file.events[0].key, 0x20);
        assert_eq!(macro_file.events[0].state, 1);
        assert_eq!(macro_file.events[0].delay_ms, 8);
        assert_eq!(macro_file.events[1].key, 0xA2);
    }

    #[test]
    fn jmm_roundtrip() {
        let bytes = include_bytes!("../../../fixtures/abc.jmm");
        let macro_file = SoftwareMacro::parse_jmm(bytes).unwrap();
        let encoded = macro_file.to_jmm().unwrap();
        let decoded = SoftwareMacro::parse_jmm(&encoded).unwrap();
        assert_eq!(macro_file, decoded);
    }

    #[test]
    fn hardware_roundtrip() {
        let macro_file = SoftwareMacro {
            name: "abc".to_string(),
            events: vec![
                SoftwareMacroEvent { key: 0x20, state: 1, delay_ms: 8 },
                SoftwareMacroEvent { key: 0xA2, state: 1, delay_ms: 0 },
            ],
        };
        let hardware = macro_file.to_hardware_events().unwrap();
        assert_eq!(hardware[0], [0x81, 0x2C, 0, 0, 8]);
        assert_eq!(hardware[1], [0x80, 0x01, 0, 0, 0]);
        let decoded = SoftwareMacro::from_hardware_events("abc".to_string(), &hardware).unwrap();
        assert_eq!(decoded, macro_file);
    }

    #[test]
    fn slot_encoding() {
        let macro_file = SoftwareMacro {
            name: "abc".to_string(),
            events: vec![SoftwareMacroEvent { key: 0x20, state: 1, delay_ms: 8 }],
        };
        let slot = macro_file.encode_hardware_slot().unwrap();
        assert_eq!(slot.len(), 0x180);
        assert!(slot[37..].iter().all(|value| *value == 0));
        assert_eq!(slot[0], 6);
        assert_eq!(&slot[1..7], b"a\0b\0c\0");
        assert_eq!(slot[31], 1);
        assert_eq!(slot[32], 0x81);
        assert_eq!(slot[33], 0x2C);
    }
}
