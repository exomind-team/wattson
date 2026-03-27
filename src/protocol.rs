use crate::data::DeviceProfile;
use crate::error::{Result, WattsonError};
use serde::{Deserialize, Serialize};

/// Protocol constants
const HEADER: [u8; 2] = [0x55, 0x7E];
const FOOTER: u8 = 0xAE;

/// Active query command (triggers PSU to start broadcasting)
pub const QUERY_CMD: [u8; 6] = [0x55, 0x7E, 0x02, 0x04, 0x06, 0xAE];

/// Fan mode / 风扇模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FanMode {
    Auto,
    Silent,
    Performance,
    Custom,
    Clean,
}

impl FanMode {
    pub const fn code(self) -> u8 {
        match self {
            Self::Auto => 0x00,
            Self::Silent => 0x01,
            Self::Performance => 0x02,
            Self::Custom => 0x03,
            Self::Clean => 0x04,
        }
    }

    /// `0x13` mode-frame checksum bytes are taken from the vendor protocol table.
    /// `0x13` 模式短帧的校验字节按厂商协议表字面值写入。
    pub const fn frame_checksum(self) -> u8 {
        match self {
            Self::Auto => 0x17,
            Self::Silent => 0x18,
            Self::Performance => 0x19,
            Self::Custom => 0x20,
            Self::Clean => 0x21,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto / 自动",
            Self::Silent => "silent / 静音",
            Self::Performance => "performance / 超频",
            Self::Custom => "custom / 自定义",
            Self::Clean => "clean / 清灰",
        }
    }
}

impl std::fmt::Display for FanMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Auto => "auto",
            Self::Silent => "silent",
            Self::Performance => "performance",
            Self::Custom => "custom",
            Self::Clean => "clean",
        };
        f.write_str(text)
    }
}

impl std::str::FromStr for FanMode {
    type Err = WattsonError;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "silent" => Ok(Self::Silent),
            "performance" | "perf" => Ok(Self::Performance),
            "custom" => Ok(Self::Custom),
            "clean" => Ok(Self::Clean),
            _ => Err(WattsonError::Protocol {
                message: format!("unknown fan mode: {value} / 未知风扇模式: {value}"),
            }),
        }
    }
}

/// Build the active query frame / 构造主动查询帧
pub fn build_query_frame() -> Vec<u8> {
    QUERY_CMD.to_vec()
}

/// Build a fan mode frame / 构造风扇模式写入帧
pub fn build_fan_mode_frame(mode: FanMode) -> Vec<u8> {
    build_frame(0x13, &[0x00, mode.code()], mode.frame_checksum())
}

/// CRC-8 (poly `0x07`, init `0x00`) used by the vendor app for `0x1B`.
/// `0x1B` 长帧使用标准 CRC-8（poly=`0x07`, init=`0x00`）。
pub fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &byte in bytes {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// Expand fan curve points into the 21-sample table used by command `0x1B`.
/// 将风扇曲线点展开为 `0x1B` 需要的 21 个采样点（0C..100C，每 5C 一点）。
pub fn expand_fan_curve_table(points: &[(u8, u8)]) -> Result<Vec<u8>> {
    let normalized = normalize_fan_curve_points(points)?;
    let samples = (0..=20)
        .map(|index| interpolate_pwm((index * 5) as u8, &normalized))
        .collect();
    Ok(samples)
}

/// Build a custom curve frame / 构造自定义风扇曲线写入帧
pub fn build_fan_curve_frame(points: &[(u8, u8)]) -> Result<Vec<u8>> {
    let normalized = normalize_fan_curve_points(points)?;
    let samples = expand_fan_curve_table(points)?;

    let mut crc_input = Vec::with_capacity(29);
    crc_input.push(0x1D);
    crc_input.push(0x1B);
    crc_input.extend(samples.iter().copied());
    for &(temp, pwm) in &normalized[1..4] {
        crc_input.push(temp);
        crc_input.push(pwm);
    }

    let checksum = crc8(&crc_input);

    let mut frame = build_frame(0x1B, &samples, checksum);
    let insert_at = 4 + samples.len();
    frame.splice(
        insert_at..insert_at,
        normalized[1..4]
            .iter()
            .flat_map(|(temp, pwm)| [*temp, *pwm]),
    );
    frame[2] = 0x1D;
    Ok(frame)
}

fn build_frame(command: u8, payload: &[u8], checksum: u8) -> Vec<u8> {
    let len = payload.len() + 2;
    let mut frame = Vec::with_capacity(2 + 1 + len + 1);
    frame.extend_from_slice(&HEADER);
    frame.push(len as u8);
    frame.push(command);
    frame.extend_from_slice(payload);
    frame.push(checksum);
    frame.push(FOOTER);
    frame
}

fn normalize_fan_curve_points(points: &[(u8, u8)]) -> Result<Vec<(u8, u8)>> {
    let mut normalized = points.to_vec();

    match normalized.len() {
        3 => {
            normalized.sort_by_key(|(temp, _)| *temp);
            validate_fan_curve_points(&normalized, true)?;
            normalized.insert(0, (0, 0));
            normalized.push((100, 100));
        }
        5 => {
            normalized.sort_by_key(|(temp, _)| *temp);
            validate_fan_curve_points(&normalized, false)?;
            if normalized.first().map(|point| point.0) != Some(0) {
                return Err(WattsonError::Protocol {
                    message: "fan curve must start at 0C / 曲线起点温度必须是 0C".to_string(),
                });
            }
            if normalized.last().map(|point| point.0) != Some(100) {
                return Err(WattsonError::Protocol {
                    message: "fan curve must end at 100C / 曲线终点温度必须是 100C".to_string(),
                });
            }
        }
        _ => {
            return Err(WattsonError::Protocol {
                message: "fan curve expects 3 control points or 5 full points / 曲线只接受 3 个控制点或 5 个完整点".to_string(),
            })
        }
    }

    Ok(normalized)
}

fn validate_fan_curve_points(points: &[(u8, u8)], interior_only: bool) -> Result<()> {
    if points.is_empty() {
        return Err(WattsonError::Protocol {
            message: "fan curve points cannot be empty / 曲线点不能为空".to_string(),
        });
    }

    let mut previous_temp = None;
    for (index, &(temp, pwm)) in points.iter().enumerate() {
        if temp > 100 || pwm > 100 {
            return Err(WattsonError::Protocol {
                message: format!(
                    "fan curve point #{index} must be within 0..=100 / 第 {index} 个曲线点必须落在 0..=100"
                ),
            });
        }

        if interior_only && (temp == 0 || temp == 100) {
            return Err(WattsonError::Protocol {
                message: "3-point input only accepts interior temperatures / 3 点输入只接受中间控制点温度".to_string(),
            });
        }

        if let Some(previous) = previous_temp {
            if temp <= previous {
                return Err(WattsonError::Protocol {
                    message:
                        "fan curve temperatures must be strictly increasing / 曲线温度必须严格递增"
                            .to_string(),
                });
            }
        }
        previous_temp = Some(temp);
    }

    Ok(())
}

fn interpolate_pwm(temp: u8, points: &[(u8, u8)]) -> u8 {
    if temp <= points[0].0 {
        return points[0].1;
    }

    for window in points.windows(2) {
        let (t0, p0) = window[0];
        let (t1, p1) = window[1];
        if temp <= t1 {
            if t1 == t0 {
                return p1;
            }

            let ratio = (temp - t0) as f64 / (t1 - t0) as f64;
            let pwm = p0 as f64 + (p1 as f64 - p0 as f64) * ratio;
            return pwm.round().clamp(0.0, 100.0) as u8;
        }
    }

    points.last().map(|(_, pwm)| *pwm).unwrap_or(0)
}

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
            if !(4..=200).contains(&pkt_len) {
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

    let u16_le =
        |offset: usize| -> u16 { u16::from_le_bytes([payload[offset], payload[offset + 1]]) };

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

    log::trace!(
        "0x04 packet: mode=0x{:02x}, len={}, hex={}",
        mode_byte,
        payload.len(),
        payload
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join(" ")
    );

    let u16_be = |i: usize| -> u16 { u16::from_be_bytes([data[i * 2], data[i * 2 + 1]]) };

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

/// Parse 0x03 combined device info packet.
///
/// The 0x03 packet is a combined frame containing model, serial, and manufacturer:
/// `[0x03][model_ascii...][0x20 0x0a]["Sn"][serial_bytes][0x0a]["G"][manufacturer...]`
///
/// Returns (model, serial_hex, manufacturer) where available.
pub fn parse_model(payload: &[u8]) -> Option<String> {
    if payload.is_empty() || payload[0] != 0x03 {
        return None;
    }
    // Extract model: take ASCII printable chars until first control char or non-printable
    let model: String = payload[1..]
        .iter()
        .take_while(|&&b| (0x20..0x7F).contains(&b)) // printable ASCII only
        .map(|&b| b as char)
        .collect::<String>()
        .trim()
        .to_string();
    if model.is_empty() {
        None
    } else {
        Some(model)
    }
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
    if text.is_empty() {
        // Try extracting serial from 0x03 combined packet as fallback
        None
    } else {
        Some(text)
    }
}

/// Try to extract serial number from a 0x03 combined packet.
/// Looks for "Sn" marker followed by binary serial data.
pub fn parse_serial_from_model_packet(payload: &[u8]) -> Option<String> {
    if payload.is_empty() || payload[0] != 0x03 {
        return None;
    }
    // Find "Sn" marker in payload
    for i in 1..payload.len().saturating_sub(4) {
        if payload[i] == b'S' && payload[i + 1] == b'n' {
            // Serial bytes follow "Sn" until next 0x0a or end
            let serial_start = i + 2;
            let serial_end = payload[serial_start..]
                .iter()
                .position(|&b| b == 0x0a)
                .map(|p| serial_start + p)
                .unwrap_or(payload.len());
            let serial_bytes = &payload[serial_start..serial_end];
            if !serial_bytes.is_empty() {
                return Some(
                    serial_bytes
                        .iter()
                        .map(|b| format!("{:02X}", b))
                        .collect::<Vec<_>>()
                        .join(""),
                );
            }
        }
    }
    None
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

    #[test]
    fn test_parse_model_combined_packet() {
        // Real 0x03 packet from DM-1000GD: model + serial + manufacturer
        let payload = [
            0x03, 0x44, 0x4d, 0x2d, 0x31, 0x30, 0x30, 0x30, 0x47, 0x44, 0x20, 0x0a, 0x53, 0x6e,
            0xf0, 0x03, 0xe8, 0x0a, 0x47, 0x73, 0x65, 0x67, 0x6f, 0x74,
        ];
        let model = parse_model(&payload);
        assert_eq!(model, Some("DM-1000GD".to_string()));

        let serial = parse_serial_from_model_packet(&payload);
        assert_eq!(serial, Some("F003E8".to_string()));
    }
}
