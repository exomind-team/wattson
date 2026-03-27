use std::collections::VecDeque;

use chrono::{DateTime, Utc};

use crate::data::PsuSnapshot;
use crate::history::History;

const MAX_HISTORY_SAMPLES: usize = 4096;

/// Snapshot with timestamp / 带时间戳的快照
#[derive(Debug, Clone)]
pub struct TimedSnapshot {
    pub timestamp: DateTime<Utc>,
    pub snapshot: PsuSnapshot,
}

/// Derived runtime stats / 运行时统计
#[derive(Debug, Clone)]
pub struct RuntimeStats {
    pub total_kwh: f64,
    pub total_cost: f64,
    pub currency: String,
    pub average_ac_input_w: f64,
    pub average_dc_output_w: f64,
    pub duration_s: f64,
}

/// All-time stats (history + current session) / 全历史统计（历史 + 当前会话）
#[derive(Debug, Clone)]
pub struct AllTimeStats {
    pub total_kwh: f64,
    pub total_cost: f64,
    pub currency: String,
    pub average_ac_input_w: f64,
    pub duration_s: f64,
}

/// GUI-facing runtime state / 面向 GUI 的运行时状态
pub struct RuntimeState {
    history: VecDeque<TimedSnapshot>,
    session_wh: f64,
    session_dc_wh: f64,
    price_per_kwh: f64,
    currency: String,
}

impl RuntimeState {
    pub fn new(price_per_kwh: f64, currency: impl Into<String>) -> Self {
        Self {
            history: VecDeque::with_capacity(512),
            session_wh: 0.0,
            session_dc_wh: 0.0,
            price_per_kwh,
            currency: currency.into(),
        }
    }

    pub fn push_snapshot(&mut self, timestamp: DateTime<Utc>, snapshot: PsuSnapshot) {
        if let Some(previous) = self.history.back() {
            let elapsed = timestamp - previous.timestamp;
            if let Ok(elapsed_std) = elapsed.to_std() {
                let elapsed_h = elapsed_std.as_secs_f64() / 3600.0;
                self.session_wh += previous.snapshot.power.ac_input_w * elapsed_h;
                self.session_dc_wh += previous.snapshot.power.dc_output_est_w * elapsed_h;
            }
        }

        self.history.push_back(TimedSnapshot {
            timestamp,
            snapshot,
        });

        while self.history.len() > MAX_HISTORY_SAMPLES {
            self.history.pop_front();
        }
    }

    pub fn latest(&self) -> Option<&TimedSnapshot> {
        self.history.back()
    }

    pub fn samples_in_window(&self, window_s: i64) -> Vec<TimedSnapshot> {
        let Some(latest) = self.history.back() else {
            return Vec::new();
        };

        self.history
            .iter()
            .filter(|sample| (latest.timestamp - sample.timestamp).num_seconds() <= window_s)
            .cloned()
            .collect()
    }

    pub fn stats(&self) -> RuntimeStats {
        let duration_s = self.session_duration_s();
        let duration_h = duration_s / 3600.0;

        let average_ac_input_w = if duration_h > 0.0 {
            self.session_wh / duration_h
        } else {
            self.latest()
                .map(|sample| sample.snapshot.power.ac_input_w)
                .unwrap_or(0.0)
        };
        let average_dc_output_w = if duration_h > 0.0 {
            self.session_dc_wh / duration_h
        } else {
            self.latest()
                .map(|sample| sample.snapshot.power.dc_output_est_w)
                .unwrap_or(0.0)
        };

        let total_kwh = self.session_wh / 1000.0;

        RuntimeStats {
            total_kwh,
            total_cost: total_kwh * self.price_per_kwh,
            currency: self.currency.clone(),
            average_ac_input_w,
            average_dc_output_w,
            duration_s,
        }
    }

    pub fn all_time_stats(&self, history: &History) -> AllTimeStats {
        let total_wh = history.total_wh + self.session_wh;
        let total_duration_s = history.total_duration_s + self.session_duration_s();
        let total_duration_h = total_duration_s / 3600.0;
        let total_kwh = total_wh / 1000.0;
        let average_ac_input_w = if total_duration_h > 0.0 {
            total_wh / total_duration_h
        } else {
            self.latest()
                .map(|sample| sample.snapshot.power.ac_input_w)
                .unwrap_or(0.0)
        };

        AllTimeStats {
            total_kwh,
            total_cost: total_kwh * self.price_per_kwh,
            currency: self.currency.clone(),
            average_ac_input_w,
            duration_s: total_duration_s,
        }
    }

    pub fn session_wh(&self) -> f64 {
        self.session_wh
    }

    pub fn session_duration_s(&self) -> f64 {
        self.history
            .front()
            .zip(self.history.back())
            .and_then(|(first, last)| (last.timestamp - first.timestamp).to_std().ok())
            .map(|duration| duration.as_secs_f64())
            .unwrap_or(0.0)
    }
}

/// Deterministic demo waveform / 可重复的演示波形
pub struct DemoGenerator;

impl DemoGenerator {
    pub fn sample_at(step: u64) -> PsuSnapshot {
        let t = step as f64;
        let ac_input_w = 162.0 + (t / 6.0).sin() * 22.0 + (t / 13.0).cos() * 9.0;
        let dc_output_w = ac_input_w * 0.885 + (t / 8.0).sin() * 3.5;
        let temp_main_c = 39.0 + (t / 20.0).sin() * 4.5;
        let temp_air_c = 31.0 + (t / 22.0).cos() * 2.5;
        let temp_air2_c = 29.0 + (t / 18.0).sin() * 2.0;
        let fan_rpm = (980.0 + (t / 7.5).sin() * 120.0).round().max(0.0) as u32;

        let mut snapshot = PsuSnapshot::default();
        snapshot.device.model = "Wattson Demo PSU".to_string();
        snapshot.device.serial = "DEMO-0001".to_string();
        snapshot.power.ac_input_w = ac_input_w;
        snapshot.power.ac_input_avg_w = ac_input_w;
        snapshot.power.dc_output_est_w = dc_output_w;
        snapshot.power.efficiency_pct = (dc_output_w / ac_input_w * 100.0).clamp(0.0, 99.9);
        snapshot.ac.voltage_v = 229.5 + (t / 30.0).sin() * 1.5;
        snapshot.ac.frequency_hz = 50.0 + (t / 40.0).cos() * 0.3;
        snapshot.dc.volt_12v = 12.05 + (t / 15.0).sin() * 0.06;
        snapshot.dc.volt_5v = 5.01 + (t / 12.0).cos() * 0.03;
        snapshot.dc.volt_3v3 = 3.31 + (t / 10.0).sin() * 0.02;
        snapshot.dc.volt_5vsb = 5.04;
        snapshot.dc.curr_12v_a = dc_output_w / 12.0;
        snapshot.dc.curr_5v_a = 0.72 + (t / 9.0).sin() * 0.08;
        snapshot.dc.curr_3v3_a = 0.24 + (t / 11.0).cos() * 0.04;
        snapshot.dc.power_12v_w = snapshot.dc.curr_12v_a * snapshot.dc.volt_12v;
        snapshot.dc.power_5v_w = snapshot.dc.curr_5v_a * snapshot.dc.volt_5v;
        snapshot.dc.power_3v3_w = snapshot.dc.curr_3v3_a * snapshot.dc.volt_3v3;
        snapshot.thermal.temp_main_c = temp_main_c;
        snapshot.thermal.temp_air_c = temp_air_c;
        snapshot.thermal.temp_air2_c = temp_air2_c;
        snapshot.fan.rpm = fan_rpm;
        snapshot.fan.pwm = 48;
        snapshot.meta.connected = true;
        snapshot.meta.packet_count = step + 1;
        snapshot.meta.poll_ms = 300;
        snapshot
    }
}
