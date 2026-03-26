use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serialport::SerialPort;

use crate::data::{DeviceProfile, PsuSnapshot};
use crate::error::{Result, WattsonError};
use crate::protocol::{self, PacketType, QUERY_CMD};

/// Communication mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    /// Listen for PSU broadcasts without sending commands
    Passive,
    /// Periodically send query commands to request data
    Active,
}

/// Shared state between reader thread and consumer
type SharedState = Arc<Mutex<PsuState>>;

struct PsuState {
    snapshot: PsuSnapshot,
    ac_history: Vec<f64>,
    dc_history: Vec<f64>,
    last_update: Option<Instant>,
}

impl PsuState {
    fn new() -> Self {
        Self {
            snapshot: PsuSnapshot::default(),
            ac_history: Vec::with_capacity(16),
            dc_history: Vec::with_capacity(16),
            last_update: None,
        }
    }
}

/// Handle to a running PSU monitor
pub struct PsuHandle {
    state: SharedState,
    stop: Arc<Mutex<bool>>,
    _thread: thread::JoinHandle<()>,
}

impl PsuHandle {
    /// Get the latest PSU data snapshot
    pub fn latest(&self) -> PsuSnapshot {
        let state = self.state.lock().unwrap();
        let mut snap = state.snapshot.clone();
        snap.meta.data_age_s = state
            .last_update
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(f64::INFINITY);
        snap
    }

    /// Check if the monitor is connected
    pub fn is_connected(&self) -> bool {
        self.state.lock().unwrap().snapshot.meta.connected
    }

    /// Stop the monitor
    pub fn stop(self) {
        *self.stop.lock().unwrap() = true;
    }
}

/// PSU Monitor — main entry point
pub struct PsuMonitor {
    port: String,
    baud: u32,
    mode: Mode,
    profile: DeviceProfile,
    poll_interval: Duration,
}

impl PsuMonitor {
    /// Create a new PSU monitor
    pub fn new(port: &str, mode: Mode) -> Self {
        Self {
            port: port.to_string(),
            baud: 115200,
            mode,
            profile: DeviceProfile::default(),
            poll_interval: Duration::from_millis(500),
        }
    }

    /// Set device profile for calibration
    pub fn with_profile(mut self, profile: DeviceProfile) -> Self {
        self.profile = profile;
        self
    }

    /// Set poll interval for active mode
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Start the monitor in a background thread
    pub fn start(self) -> Result<PsuHandle> {
        let state: SharedState = Arc::new(Mutex::new(PsuState::new()));
        let stop = Arc::new(Mutex::new(false));

        let state_clone = state.clone();
        let stop_clone = stop.clone();

        let thread = thread::Builder::new()
            .name("wattson-reader".into())
            .spawn(move || {
                reader_loop(
                    self.port,
                    self.baud,
                    self.mode,
                    self.profile,
                    self.poll_interval,
                    state_clone,
                    stop_clone,
                );
            })
            .map_err(WattsonError::Io)?;

        Ok(PsuHandle {
            state,
            stop,
            _thread: thread,
        })
    }
}

fn reader_loop(
    port: String,
    baud: u32,
    mode: Mode,
    profile: DeviceProfile,
    poll_interval: Duration,
    state: SharedState,
    stop: Arc<Mutex<bool>>,
) {
    while !*stop.lock().unwrap() {
        match serialport::new(&port, baud)
            .timeout(Duration::from_secs(2))
            .open()
        {
            Ok(mut serial) => {
                {
                    let mut s = state.lock().unwrap();
                    s.snapshot.meta.connected = true;
                }
                log::info!("Connected to {}", port);

                if mode == Mode::Active {
                    let _ = serial.write(&QUERY_CMD);
                }

                if let Err(e) =
                    read_frames(&mut *serial, mode, &profile, poll_interval, &state, &stop)
                {
                    log::warn!("Read error: {}", e);
                    let mut s = state.lock().unwrap();
                    s.snapshot.meta.connected = false;
                }
            }
            Err(e) => {
                let mut s = state.lock().unwrap();
                s.snapshot.meta.connected = false;
                let msg = format!("{}", e);
                if msg.contains("Access") || msg.contains("拒绝") || msg.contains("PermissionError")
                {
                    s.snapshot.meta.error_count += 1;
                    log::error!("Port {} is busy (another program may be using it, e.g. HiMOS). Close that program first.", port);
                } else {
                    log::error!("Cannot open {}: {}", port, e);
                }
            }
        }

        if !*stop.lock().unwrap() {
            thread::sleep(Duration::from_secs(2));
        }
    }
}

fn read_frames(
    serial: &mut dyn SerialPort,
    mode: Mode,
    profile: &DeviceProfile,
    poll_interval: Duration,
    state: &SharedState,
    stop: &Arc<Mutex<bool>>,
) -> Result<()> {
    let mut buf = Vec::with_capacity(4096);
    let mut read_buf = [0u8; 512];
    let mut last_query = Instant::now();

    while !*stop.lock().unwrap() {
        // Read available bytes
        match serial.read(&mut read_buf) {
            Ok(n) if n > 0 => buf.extend_from_slice(&read_buf[..n]),
            Ok(_) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => return Err(WattsonError::Io(e)),
        }

        // Active mode: periodic query
        if mode == Mode::Active && last_query.elapsed() > poll_interval {
            let _ = serial.write(&QUERY_CMD);
            last_query = Instant::now();
        }

        // Parse all complete frames
        while let Some((payload, consumed)) = protocol::find_frame(&buf) {
            if !payload.is_empty() {
                process_payload(&payload, profile, state);
            }
            buf = buf[consumed..].to_vec();
        }

        // Prevent buffer from growing unbounded
        if buf.len() > 8192 {
            buf.drain(..buf.len() - 1024);
        }
    }

    Ok(())
}

fn process_payload(payload: &[u8], profile: &DeviceProfile, state: &SharedState) {
    let pkt_type = PacketType::from(payload[0]);
    let mut s = state.lock().unwrap();

    match pkt_type {
        PacketType::Electrical => {
            if let Some(data) = protocol::parse_electrical(payload, profile) {
                let dc = &mut s.snapshot.dc;
                dc.volt_12v = data.volt_12v;
                dc.volt_5v = data.volt_5v;
                dc.volt_3v3 = data.volt_3v3;
                dc.volt_5vsb = data.volt_5vsb;
                dc.curr_12v_a = data.curr_12v;
                dc.curr_5v_a = data.curr_5v;
                dc.curr_3v3_a = data.curr_3v3;
                dc.power_12v_w = data.volt_12v * data.curr_12v;
                dc.power_5v_w = data.volt_5v * data.curr_5v;
                dc.power_3v3_w = data.volt_3v3 * data.curr_3v3;

                let dc_total = dc.power_12v_w + dc.power_5v_w + dc.power_3v3_w;
                s.snapshot.power.dc_output_est_w = dc_total;

                s.snapshot.ac.voltage_v = data.ac_voltage;
                s.snapshot.ac.frequency_hz = data.ac_freq;
                s.snapshot.fan.rpm = data.fan_rpm;

                // DC sliding average
                s.dc_history.push(dc_total);
                if s.dc_history.len() > 10 {
                    s.dc_history.remove(0);
                }

                s.snapshot.meta.packet_count += 1;
                s.last_update = Some(Instant::now());
            }
        }
        PacketType::ExtendedStatus => {
            if let Some(data) = protocol::parse_extended(payload, profile) {
                s.snapshot.thermal.temp_main_c = data.temp_main;
                s.snapshot.thermal.temp_air_c = data.temp_air;
                s.snapshot.thermal.temp_air2_c = data.temp_air2;
                s.snapshot.fan.pwm = data.mode_byte;
                s.snapshot.power.ac_input_w = data.ac_power;

                // AC sliding average
                s.ac_history.push(data.ac_power);
                if s.ac_history.len() > 10 {
                    s.ac_history.remove(0);
                }
                let ac_avg: f64 = s.ac_history.iter().sum::<f64>() / s.ac_history.len() as f64;
                s.snapshot.power.ac_input_avg_w = ac_avg;

                // Efficiency
                let dc_avg: f64 = if s.dc_history.is_empty() {
                    0.0
                } else {
                    s.dc_history.iter().sum::<f64>() / s.dc_history.len() as f64
                };
                if dc_avg > 0.0 && ac_avg > 0.0 {
                    let eff = (dc_avg / ac_avg) * 100.0;
                    s.snapshot.power.efficiency_pct = eff.min(99.9);
                }

                s.snapshot.meta.packet_count += 1;
                s.last_update = Some(Instant::now());
            }
        }
        PacketType::DeviceModel => {
            if let Some(model) = protocol::parse_model(payload) {
                s.snapshot.device.model = model;
            }
        }
        PacketType::SerialNumber => {
            if let Some(serial) = protocol::parse_serial(payload) {
                s.snapshot.device.serial = serial;
            }
        }
        PacketType::Unknown(_) => {
            s.snapshot.meta.error_count += 1;
        }
    }
}
