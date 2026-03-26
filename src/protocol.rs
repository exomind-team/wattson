use crate::data::DeviceProfile;

/// Protocol constants
const HEADER: [u8; 2] = [0x55, 0x7E];
const _FOOTER: u8 = 0xAE;

/// Active query command (triggers PSU to start broadcasting)
pub const QUERY_CMD: [u8; 6] = [0x55, 0x7E, 0x02, 0x04, 0x06, 0xAE];

/// Packet type identifiers
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PacketType {
    /// 0x02: Electrical parameters (voltages, currents, AC input)
    Electrical,
    /// 0x03: Device model string
    DeviceModel,
    /// 0x04: Extended status (temperature, fan mode, AC power)
    ExtendedStatus,
    /// 0x05: Serial number
    SerialNumber,
    /// Unknown packet type
    Unknown(u8),
}

impl From<u8> for PacketType {
    fn from(b: u8) -> Self {
        match b {
            0x02 => Self::Electrical,
            0x03 => Self::DeviceModel,
            0x04 => Self::ExtendedStatus,
            0x05 => Self::SerialNumber,
            _ => Self::Unknown(b),
        }
    }
}

/// Parsed electrical data from 0x02 packet
#[derive(Debug, Default)]
pub struct ElectricalData {
    pub volt_3v3: f64,
    pub volt_5v: f64,
    pub volt_12v: f64,
    pub volt_5vsb: f64,
    pub curr_3v3: f64,
    pub curr_5v: f64,
    pub curr_12v: f64,
    pub ac_freq: f64,
    pub ac_voltage: f64,
    pub fan_rpm: u32,
}

/// Parsed extended status from 0x04 packet
#[derive(Debug, Default)]
pub struct ExtendedData {
    pub mode_byte: u8,
    pub temp_main: f64,
    pub ac_power: f64,
    pub temp_air: f64,
    pub temp_air2: f64,
}

/// Find the next valid frame in a byte buffer.
/// Returns (frame_payload, bytes_consumed) or None.
pub fn find_frame(buf: &[u8]) -> Option<(Vec<u8>, usize)> {
    let mut i = 0;
    while i < buf.len().saturating_sub(4) {
        if buf[i] == HEADER[0] && buf[i + 1] == HEADER[1] {
            let pkt_len = buf[i + 2] as usize;
            if pkt_len < 4 || pkt_len > 200 {
                i += 1;
                continue;
            }
            let frame_end = i + 3 + pkt_len;
            if frame_end > buf.len() {
                return None; // incomplete frame, need more data
            }
            // Extract payload (exclude header, length, checksum, footer)
            let payload = buf[i + 3..i + 3 + pkt_len - 3].to_vec();
            return Some((payload, frame_end));
        }
        i += 1;
    }
    None
}

/// Parse 0x02 electrical parameters packet (little-endian uint16)
pub fn parse_electrical(payload: &[u8], profile: &DeviceProfile) -> Option<ElectricalData> {
    if payload.len() < 27 || payload[0] != 0x02 {
        return None;
    }

    let u16_le = |offset: usize| -> u16 {
        u16::from_le_bytes([payload[offset], payload[offset + 1]])
    };

    let raw: Vec<u16> = (0..13).map(|i| u16_le(1 + i * 2)).collect();

    Some(ElectricalData {
        volt_3v3: raw[0] as f64 / 1000.0,
        volt_5v: raw[1] as f64 / 1000.0,
        volt_12v: raw[2] as f64 / 1000.0,
        volt_5vsb: raw[3] as f64 / 1000.0,
        curr_3v3: raw[4] as f64 / profile.curr_3v3_divisor,
        curr_5v: raw[5] as f64 / profile.curr_5v_divisor,
        curr_12v: raw[6] as f64 / profile.curr_12v_divisor,
        ac_freq: raw[7] as f64 / 10.0,
        ac_voltage: raw[11] as f64 / 10.0,
        fan_rpm: raw[12] as u32 * 30,
    })
}

/// Parse 0x04 extended status packet (big-endian uint16)
pub fn parse_extended(payload: &[u8], profile: &DeviceProfile) -> Option<ExtendedData> {
    if payload.len() < 16 || payload[0] != 0x04 {
        return None;
    }

    let mode_byte = payload[1];
    let data = &payload[2..];
    let num = data.len() / 2;

    if num < profile.ac_power_index + 1 {
        return None;
    }

    let u16_be = |i: usize| -> u16 {
        u16::from_be_bytes([data[i * 2], data[i * 2 + 1]])
    };

    let mut result = ExtendedData {
        mode_byte,
        temp_main: u16_be(0) as f64 / 10.0,
        ac_power: u16_be(profile.ac_power_index) as f64 / 10.0,
        ..Default::default()
    };

    if num >= 11 {
        result.temp_air = u16_be(10) as f64 / 100.0;
    }
    if num >= 12 {
        result.temp_air2 = u16_be(11) as f64 / 100.0;
    }

    Some(result)
}

/// Parse 0x03 device model string
pub fn parse_model(payload: &[u8]) -> Option<String> {
    if payload.is_empty() || payload[0] != 0x03 {
        return None;
    }
    let text = String::from_utf8_lossy(&payload[1..])
        .trim_matches('\0')
        .trim()
        .to_string();
    if text.is_empty() { None } else { Some(text) }
}

/// Parse 0x05 serial number string
pub fn parse_serial(payload: &[u8]) -> Option<String> {
    if payload.is_empty() || payload[0] != 0x05 {
        return None;
    }
    let text = String::from_utf8_lossy(&payload[1..])
        .trim_matches('\0')
        .trim()
        .to_string();
    if text.is_empty() { None } else { Some(text) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_frame() {
        let data = [0x55, 0x7E, 0x06, 0x02, 0x01, 0x02, 0xCC, 0xDD, 0xAE];
        let result = find_frame(&data);
        assert!(result.is_some());
        let (payload, consumed) = result.unwrap();
        assert_eq!(payload, vec![0x02, 0x01, 0x02]);
        assert_eq!(consumed, 9);
    }

    #[test]
    fn test_parse_model() {
        let payload = [0x03, b'D', b'M', b'-', b'1', b'0', b'0', b'0', b'G', 0x00];
        let model = parse_model(&payload);
        assert_eq!(model, Some("DM-1000G".to_string()));
    }
}
