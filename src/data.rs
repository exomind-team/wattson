use serde::Serialize;

/// Complete PSU data snapshot
#[derive(Debug, Clone, Default, Serialize)]
pub struct PsuSnapshot {
    pub device: DeviceInfo,
    pub power: PowerData,
    pub ac: AcData,
    pub dc: DcData,
    pub thermal: ThermalData,
    pub fan: FanData,
    pub cost: CostData,
    pub meta: MetaData,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DeviceInfo {
    pub model: String,
    pub serial: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PowerData {
    /// AC input power in watts (primary metric)
    pub ac_input_w: f64,
    /// Smoothed AC input power (sliding average)
    pub ac_input_avg_w: f64,
    /// Estimated DC output power in watts
    pub dc_output_est_w: f64,
    /// Estimated conversion efficiency (percentage)
    pub efficiency_pct: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AcData {
    pub voltage_v: f64,
    pub frequency_hz: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DcData {
    pub volt_12v: f64,
    pub volt_5v: f64,
    pub volt_3v3: f64,
    pub volt_5vsb: f64,
    pub curr_12v_a: f64,
    pub curr_5v_a: f64,
    pub curr_3v3_a: f64,
    pub power_12v_w: f64,
    pub power_5v_w: f64,
    pub power_3v3_w: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ThermalData {
    pub temp_main_c: f64,
    pub temp_air_c: f64,
    pub temp_air2_c: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FanData {
    pub rpm: u32,
    pub pwm: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct CostData {
    pub total_kwh: f64,
    pub total_cost: f64,
    pub currency: String,
    pub price_per_kwh: f64,
    pub monitoring_duration_s: f64,
}

impl Default for CostData {
    fn default() -> Self {
        Self {
            total_kwh: 0.0,
            total_cost: 0.0,
            currency: "CNY".to_string(),
            price_per_kwh: 0.56,
            monitoring_duration_s: 0.0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MetaData {
    pub packet_count: u64,
    pub error_count: u64,
    pub connected: bool,
    /// Seconds since last valid data
    pub data_age_s: f64,
    /// Current poll interval in ms (for display)
    #[serde(skip_serializing_if = "is_zero_u64")]
    pub poll_ms: u64,
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

/// Device-specific calibration profile
#[derive(Debug, Clone)]
pub struct DeviceProfile {
    pub name: &'static str,
    /// Divisor for 12V current raw value
    pub curr_12v_divisor: f64,
    /// Divisor for 5V current raw value
    pub curr_5v_divisor: f64,
    /// Divisor for 3.3V current raw value
    pub curr_3v3_divisor: f64,
    /// Index of AC power field in 0x04 packet (big-endian uint16 array)
    pub ac_power_index: usize,
}

impl DeviceProfile {
    /// Segotep DM-850G (tested by YveMU)
    pub const DM850G: Self = Self {
        name: "Segotep DM-850G",
        curr_12v_divisor: 256.0,
        curr_5v_divisor: 4096.0,
        curr_3v3_divisor: 4096.0,
        ac_power_index: 6,
    };

    /// Segotep DM-1000G (calibrated via reverse engineering, 2026-03)
    pub const DM1000G: Self = Self {
        name: "Segotep DM-1000G",
        curr_12v_divisor: 300.0,
        curr_5v_divisor: 4096.0,
        curr_3v3_divisor: 4096.0,
        ac_power_index: 6,
    };

    /// Generic Segotep DM series (uses DM-1000G calibration)
    pub const SEGOTEP_DM: Self = Self::DM1000G;
}

impl Default for DeviceProfile {
    fn default() -> Self {
        Self::SEGOTEP_DM
    }
}
