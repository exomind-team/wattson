use std::io::Write;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serialport::SerialPort;

use crate::data::{DeviceProfile, PsuSnapshot};
use crate::error::{Result, WattsonError};
use crate::protocol::{self, FanMode, PacketType, QUERY_CMD};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SerialCommand {
    Mode(FanMode),
    Curve(Vec<(u8, u8)>),
    Pwm(u8),
}

struct CommandRequest {
    command: SerialCommand,
    reply_tx: Sender<Result<()>>,
}

struct PsuState {
    snapshot: PsuSnapshot,
    ac_history: Vec<f64>,
    dc_history: Vec<f64>,
    ac_ema: Option<f64>,
    last_update: Option<Instant>,
}

/// EMA smoothing factor (0.0–1.0, higher = tracks faster)
/// 0.5 balances smoothing quantization noise while tracking real changes
const AC_EMA_ALPHA: f64 = 0.5;

impl PsuState {
    fn new() -> Self {
        Self {
            snapshot: PsuSnapshot::default(),
            ac_history: Vec::with_capacity(16),
            dc_history: Vec::with_capacity(16),
            ac_ema: None,
            last_update: None,
        }
    }
}

/// Handle to a running PSU monitor
pub struct PsuHandle {
    state: SharedState,
    stop: Arc<Mutex<bool>>,
    poll_ms: Arc<Mutex<u64>>,
    command_tx: Sender<CommandRequest>,
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
        snap.meta.poll_ms = *self.poll_ms.lock().unwrap();
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

    /// Get current poll interval in milliseconds
    pub fn poll_ms(&self) -> u64 {
        *self.poll_ms.lock().unwrap()
    }

    /// Set poll interval in milliseconds (min 100, max 5000)
    pub fn set_poll_ms(&self, ms: u64) {
        let ms = ms.clamp(200, 5000);
        *self.poll_ms.lock().unwrap() = ms;
    }

    /// Set fan mode / 设置风扇模式
    pub fn set_fan_mode(&self, mode: FanMode) -> Result<()> {
        self.send_command(SerialCommand::Mode(mode))
    }

    /// Set a flat PWM curve and switch to custom mode / 设置固定占空比并切到自定义模式
    pub fn set_fan_pwm(&self, value: u8) -> Result<()> {
        self.send_command(SerialCommand::Pwm(value))
    }

    /// Set a custom fan curve and switch to custom mode / 设置自定义风扇曲线并切到自定义模式
    pub fn set_fan_curve(&self, points: Vec<(u8, u8)>) -> Result<()> {
        self.send_command(SerialCommand::Curve(points))
    }

    fn send_command(&self, command: SerialCommand) -> Result<()> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.command_tx
            .send(CommandRequest { command, reply_tx })
            .map_err(|_| WattsonError::NotConnected)?;

        reply_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| WattsonError::Timeout)?
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
            poll_interval: Duration::from_millis(300),
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
        let poll_ms = Arc::new(Mutex::new(self.poll_interval.as_millis() as u64));
        let (command_tx, command_rx) = mpsc::channel();

        let state_clone = state.clone();
        let stop_clone = stop.clone();
        let poll_ms_clone = poll_ms.clone();

        let thread = thread::Builder::new()
            .name("wattson-reader".into())
            .spawn(move || {
                reader_loop(
                    self.port,
                    self.baud,
                    self.mode,
                    self.profile,
                    poll_ms_clone,
                    state_clone,
                    stop_clone,
                    command_rx,
                );
            })
            .map_err(WattsonError::Io)?;

        Ok(PsuHandle {
            state,
            stop,
            poll_ms,
            command_tx,
            _thread: thread,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn reader_loop(
    port: String,
    baud: u32,
    mode: Mode,
    profile: DeviceProfile,
    poll_ms: Arc<Mutex<u64>>,
    state: SharedState,
    stop: Arc<Mutex<bool>>,
    commands_rx: Receiver<CommandRequest>,
) {
    while !*stop.lock().unwrap() {
        match serialport::new(&port, baud)
            .timeout(Duration::from_millis(100))
            .open()
        {
            Ok(mut serial) => {
                {
                    let mut s = state.lock().unwrap();
                    s.snapshot.meta.connected = true;
                }
                log::info!("Connected to {}", port);

                // Always send query command at startup to trigger device model/serial broadcasts
                let _ = serial.write(&QUERY_CMD);
                log::debug!("Sent initial QUERY_CMD to {}", port);

                if let Err(e) = read_frames(
                    &mut *serial,
                    mode,
                    &profile,
                    &poll_ms,
                    &state,
                    &stop,
                    &commands_rx,
                ) {
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
                reject_pending_commands(&commands_rx);
            }
        }

        if !*stop.lock().unwrap() {
            thread::sleep(Duration::from_secs(2));
        }
    }
}

fn read_frames(
    serial: &mut dyn SerialPort,
    _mode: Mode,
    profile: &DeviceProfile,
    poll_ms: &Arc<Mutex<u64>>,
    state: &SharedState,
    stop: &Arc<Mutex<bool>>,
    commands_rx: &Receiver<CommandRequest>,
) -> Result<()> {
    let mut buf = Vec::with_capacity(4096);
    let mut read_buf = [0u8; 512];
    let mut last_query = Instant::now();
    let mut model_retry_at = Some(Instant::now() + Duration::from_secs(3));

    while !*stop.lock().unwrap() {
        // Read available bytes
        match serial.read(&mut read_buf) {
            Ok(n) if n > 0 => buf.extend_from_slice(&read_buf[..n]),
            Ok(_) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => return Err(WattsonError::Io(e)),
        }

        while let Ok(request) = commands_rx.try_recv() {
            let result = process_command(serial, &request.command);
            let _ = request.reply_tx.send(result);
            last_query = Instant::now();
        }

        // Periodic query: use dynamic poll interval from shared state
        let query_interval = Duration::from_millis(*poll_ms.lock().unwrap());
        if last_query.elapsed() > query_interval {
            let _ = serial.write(&QUERY_CMD);
            last_query = Instant::now();
        }

        // Retry model query if model is still empty after 3s
        if let Some(retry_time) = model_retry_at {
            if Instant::now() >= retry_time {
                let model_empty = state.lock().unwrap().snapshot.device.model.is_empty();
                if model_empty {
                    log::debug!("Model still unknown, re-sending QUERY_CMD");
                    let _ = serial.write(&QUERY_CMD);
                    model_retry_at = Some(Instant::now() + Duration::from_secs(5));
                } else {
                    model_retry_at = None; // Got model, stop retrying
                }
            }
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

fn reject_pending_commands(commands_rx: &Receiver<CommandRequest>) {
    while let Ok(request) = commands_rx.try_recv() {
        let _ = request.reply_tx.send(Err(WattsonError::NotConnected));
    }
}

fn process_command(serial: &mut dyn SerialPort, command: &SerialCommand) -> Result<()> {
    for frame in build_frames_for_command(command)? {
        serial.write_all(&frame)?;
        serial.flush()?;
        thread::sleep(Duration::from_millis(40));
    }

    serial.write_all(&QUERY_CMD)?;
    serial.flush()?;
    Ok(())
}

fn build_frames_for_command(command: &SerialCommand) -> Result<Vec<Vec<u8>>> {
    match command {
        SerialCommand::Mode(mode) => Ok(vec![protocol::build_fan_mode_frame(*mode)]),
        SerialCommand::Curve(points) => Ok(vec![
            protocol::build_fan_curve_frame(points)?,
            protocol::build_fan_mode_frame(FanMode::Custom),
        ]),
        SerialCommand::Pwm(value) => {
            if *value > 100 {
                return Err(WattsonError::Protocol {
                    message: "fan pwm must be within 0..=100 / 风扇占空比必须在 0..=100"
                        .to_string(),
                });
            }

            let flat_curve = vec![
                (0, *value),
                (30, *value),
                (60, *value),
                (90, *value),
                (100, *value),
            ];

            Ok(vec![
                protocol::build_fan_curve_frame(&flat_curve)?,
                protocol::build_fan_mode_frame(FanMode::Custom),
            ])
        }
    }
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

                // AC power EMA smoothing (removes quantization noise without lagging real changes)
                let raw_ac = data.ac_power;
                let filtered_ac = match s.ac_ema {
                    Some(ema) => ema + (raw_ac - ema) * AC_EMA_ALPHA,
                    None => raw_ac,
                };
                s.ac_ema = Some(filtered_ac);
                s.snapshot.power.ac_input_w = filtered_ac;

                // AC sliding average (uses filtered values)
                s.ac_history.push(filtered_ac);
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
                log::info!("Device model: {}", model);
                s.snapshot.device.model = model;
            }
            // 0x03 packet also contains serial number — extract if we don't have one yet
            if s.snapshot.device.serial.is_empty() {
                if let Some(serial) = protocol::parse_serial_from_model_packet(payload) {
                    log::info!("Serial (from 0x03): {}", serial);
                    s.snapshot.device.serial = serial;
                }
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

#[cfg(test)]
pub(crate) fn test_handle_with_recorder(
    snapshot: PsuSnapshot,
) -> (PsuHandle, Receiver<SerialCommand>) {
    let mut state_value = PsuState::new();
    state_value.snapshot = snapshot;
    state_value.last_update = Some(Instant::now());

    let state = Arc::new(Mutex::new(state_value));
    let stop = Arc::new(Mutex::new(false));
    let poll_ms = Arc::new(Mutex::new(300));
    let (command_tx, command_rx) = mpsc::channel::<CommandRequest>();
    let (record_tx, record_rx) = mpsc::channel::<SerialCommand>();

    let worker = thread::spawn(move || {
        while let Ok(request) = command_rx.recv() {
            let _ = record_tx.send(request.command.clone());
            let _ = request.reply_tx.send(Ok(()));
        }
    });

    (
        PsuHandle {
            state,
            stop,
            poll_ms,
            command_tx,
            _thread: worker,
        },
        record_rx,
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::protocol::{self, FanMode};

    #[test]
    fn build_frames_for_set_fan_mode_uses_single_short_frame() {
        let frames =
            build_frames_for_command(&SerialCommand::Mode(FanMode::Custom)).expect("mode frames");

        assert_eq!(
            frames,
            vec![protocol::build_fan_mode_frame(FanMode::Custom)]
        );
    }

    #[test]
    fn build_frames_for_set_fan_pwm_writes_curve_then_custom_mode() {
        let frames = build_frames_for_command(&SerialCommand::Pwm(80)).expect("pwm frames");

        assert_eq!(frames.len(), 2);
        assert_eq!(
            frames[0],
            protocol::build_fan_curve_frame(&[(0, 80), (30, 80), (60, 80), (90, 80), (100, 80)])
                .expect("curve frame")
        );
        assert_eq!(frames[1], protocol::build_fan_mode_frame(FanMode::Custom));
    }

    #[test]
    fn handle_can_enqueue_fan_commands_to_background_worker() {
        let mut snapshot = crate::data::PsuSnapshot::default();
        snapshot.meta.connected = true;

        let (handle, receiver) = test_handle_with_recorder(snapshot);
        handle.set_fan_pwm(55).expect("send pwm command");

        let command = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("recorded command");
        assert_eq!(command, SerialCommand::Pwm(55));
    }
}
