use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Parser, Subcommand};
use wattson::config::Config;
use wattson::data::DeviceProfile;
use wattson::protocol::FanMode;
use wattson::serial::{Mode, PsuMonitor};

#[derive(Parser)]
#[command(
    name = "wattson",
    version,
    about = "Universal digital PSU monitoring tool",
    long_about = "Read real-time power consumption from your PSU via serial protocols.\nProject: https://github.com/exomind-team/wattson"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// One-shot read: capture and print PSU data as JSON
    Read {
        /// Duration in seconds to capture (default: 3)
        #[arg(long, default_value_t = 3)]
        duration: u64,
    },
    /// Continuous JSON output (one snapshot per second)
    Watch,
    /// Configuration management
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Launch interactive TUI dashboard
    Tui {
        /// Refresh interval in milliseconds
        #[arg(long, default_value_t = 200)]
        refresh: u64,
    },
    /// Launch native GUI dashboard (egui + wgpu 原生界面)
    Gui {
        /// Use deterministic demo data (演示数据)
        #[arg(long, default_value_t = false)]
        demo: bool,
    },
    /// Start HTTP API server
    Serve {
        /// Port to listen on (overrides config)
        #[arg(long)]
        port: Option<u16>,
    },
    /// Fan control / 风扇控制
    Fan {
        #[command(subcommand)]
        action: FanAction,
    },
    /// Generate chart from recorded data
    Chart {
        /// Use last N data points
        #[arg(long)]
        last: Option<usize>,
        /// Input file (JSON-lines)
        #[arg(long)]
        input: PathBuf,
        /// Output PNG file
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Show device information
    Info,
    /// Show electricity cost statistics
    Cost,
    /// List available serial ports
    Ports,
}

#[derive(Subcommand)]
enum FanAction {
    /// Set a flat PWM curve and switch to custom mode / 设置固定占空比并切到自定义模式
    Set {
        /// PWM percentage 0..100
        pwm: u8,
    },
    /// Apply a custom curve JSON like [[30,40],[50,55],[70,75]] / 应用曲线 JSON
    Curve {
        /// Curve points as JSON
        json: String,
    },
    /// Set fan mode / 设置风扇模式
    Mode {
        /// auto | silent | performance | custom | clean
        mode: String,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Show current configuration
    Show,
    /// Set a config value (e.g. serial.port COM5)
    Set {
        /// Config key (e.g. serial.port)
        key: String,
        /// New value
        value: String,
    },
    /// Generate default wattson.toml in current directory
    Init,
}

fn resolve_mode(config: &Config) -> Mode {
    if config.serial.mode == "active" {
        Mode::Active
    } else {
        Mode::Passive
    }
}

fn resolve_profile(config: &Config) -> DeviceProfile {
    match config.device.profile.to_lowercase().as_str() {
        "dm850g" | "dm-850g" | "segotep_dm850g" => DeviceProfile::DM850G,
        "dm1000g" | "dm-1000g" | "segotep_dm1000g" => DeviceProfile::DM1000G,
        _ => DeviceProfile::SEGOTEP_DM,
    }
}

fn create_monitor(config: &Config) -> PsuMonitor {
    PsuMonitor::new(&config.serial.port, resolve_mode(config)).with_profile(resolve_profile(config))
}

fn main() {
    let cli = Cli::parse();
    let mut config = Config::load();

    match cli.command.unwrap_or(Commands::Gui { demo: false }) {
        Commands::Read { duration } => cmd_read(&config, duration),
        Commands::Watch => cmd_watch(&config),
        Commands::Config { action } => cmd_config(&mut config, action),
        Commands::Tui { refresh } => cmd_tui(&config, refresh),
        Commands::Gui { demo } => cmd_gui(&config, demo),
        Commands::Serve { port } => cmd_serve(&mut config, port),
        Commands::Fan { action } => cmd_fan(&config, action),
        Commands::Chart {
            last,
            input,
            output,
        } => cmd_chart(&config, last, &input, output),
        Commands::Info => cmd_info(&config),
        Commands::Cost => cmd_cost(&config),
        Commands::Ports => cmd_ports(),
    }
}

fn cmd_read(config: &Config, duration: u64) {
    let monitor = create_monitor(config);
    let handle = match monitor.start() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    eprintln!(
        "Reading from {} for {} seconds...",
        config.serial.port, duration
    );

    // Wait for data to arrive
    let deadline = std::time::Instant::now() + Duration::from_secs(duration);
    while std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }

    let snap = handle.latest();
    if !snap.meta.connected || snap.meta.packet_count == 0 {
        eprintln!();
        eprintln!("⚠ No data received from {}!", config.serial.port);
        eprintln!("  Possible causes:");
        eprintln!("  - Port is busy (close HiMOS or other serial monitor first)");
        eprintln!("  - PSU USB cable not connected");
        eprintln!("  - Wrong port (run 'wattson ports' to check)");
        eprintln!();
    }
    println!("{}", serde_json::to_string_pretty(&snap).unwrap());
    handle.stop();
}

fn cmd_watch(config: &Config) {
    let monitor = create_monitor(config);
    let handle = match monitor.start() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    eprintln!("Streaming from {} (Ctrl+C to stop)...", config.serial.port);

    // Wait for first data
    for _ in 0..50 {
        if handle.latest().meta.packet_count > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    loop {
        let snap = handle.latest();
        println!("{}", serde_json::to_string(&snap).unwrap());
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn cmd_config(config: &mut Config, action: ConfigAction) {
    match action {
        ConfigAction::Show => {
            if let Some(path) = Config::active_path() {
                eprintln!("Config file: {}", path.display());
            } else {
                eprintln!("No config file found (using defaults)");
            }
            println!(
                "{}",
                toml::to_string_pretty(config).unwrap_or_else(|_| "Error serializing".into())
            );
        }
        ConfigAction::Set { key, value } => match config.set_value(&key, &value) {
            Ok(()) => match config.save() {
                Ok(path) => println!("Set {} = {} (saved to {})", key, value, path.display()),
                Err(e) => {
                    eprintln!("Warning: set in memory but failed to save: {}", e);
                }
            },
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        },
        ConfigAction::Init => match Config::init_default() {
            Ok(path) => println!("Created {}", path.display()),
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        },
    }
}

fn cmd_tui(config: &Config, refresh: u64) {
    let monitor = create_monitor(config);
    let handle = match monitor.start() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) = wattson::tui::run(&handle, config, refresh) {
        eprintln!("TUI error: {}", e);
        std::process::exit(1);
    }

    handle.stop();
}

fn cmd_gui(config: &Config, demo: bool) {
    let handle = if demo {
        None
    } else {
        let monitor = create_monitor(config);
        match monitor.start() {
            Ok(handle) => Some(handle),
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    };

    if let Err(e) = wattson::gui::run(config.clone(), handle, demo) {
        eprintln!("GUI error: {}", e);
        std::process::exit(1);
    }
}

fn cmd_serve(config: &mut Config, port_override: Option<u16>) {
    if let Some(p) = port_override {
        config.api.port = p;
    }

    let monitor = create_monitor(config);
    let handle = match monitor.start() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(wattson::api::serve(handle, config));
}

fn cmd_chart(config: &Config, last: Option<usize>, input: &Path, output: Option<PathBuf>) {
    let mut data = match wattson::chart::load_data_points(input) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error loading data: {}", e);
            std::process::exit(1);
        }
    };

    if let Some(n) = last {
        if n < data.len() {
            data = data.split_off(data.len() - n);
        }
    }

    let out =
        output.unwrap_or_else(|| PathBuf::from(&config.chart.output_dir).join("wattson_chart.png"));

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    // Try to get device model from the first data point... not easily available
    // so use a placeholder based on config
    let model = config.device.profile.clone();

    match wattson::chart::generate_chart(&data, &out, config, &model) {
        Ok(()) => println!("Chart saved to {}", out.display()),
        Err(e) => {
            eprintln!("Error generating chart: {}", e);
            std::process::exit(1);
        }
    }
}

fn cmd_info(config: &Config) {
    let monitor = create_monitor(config);
    let handle = match monitor.start() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    eprintln!("Querying device info from {}...", config.serial.port);

    // Wait for device model/serial packets
    for _ in 0..60 {
        let snap = handle.latest();
        if !snap.device.model.is_empty() && !snap.device.serial.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let snap = handle.latest();
    println!(
        "Model:      {}",
        if snap.device.model.is_empty() {
            "N/A"
        } else {
            &snap.device.model
        }
    );
    println!(
        "Serial:     {}",
        if snap.device.serial.is_empty() {
            "N/A"
        } else {
            &snap.device.serial
        }
    );
    println!("Connected:  {}", snap.meta.connected);
    println!("Packets:    {}", snap.meta.packet_count);
    println!("Port:       {}", config.serial.port);
    println!("Mode:       {}", config.serial.mode);
    println!("Profile:    {}", config.device.profile);

    handle.stop();
}

fn cmd_cost(config: &Config) {
    let monitor = create_monitor(config);
    let handle = match monitor.start() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    eprintln!(
        "Monitoring power from {} (Ctrl+C to stop, shows running total)...",
        config.serial.port
    );

    // Wait for data
    for _ in 0..50 {
        if handle.latest().meta.packet_count > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let start = std::time::Instant::now();
    let mut total_wh = 0.0;
    let mut last_tick = std::time::Instant::now();

    loop {
        let snap = handle.latest();
        let elapsed_h = last_tick.elapsed().as_secs_f64() / 3600.0;
        total_wh += snap.power.ac_input_w * elapsed_h;
        last_tick = std::time::Instant::now();

        let total_kwh = total_wh / 1000.0;
        let cost = total_kwh * config.cost.price_per_kwh;
        let duration = start.elapsed().as_secs();

        eprint!(
            "\r  {:.1}W | {:.4} kWh | {:.4} {} | {:02}:{:02}:{:02}   ",
            snap.power.ac_input_w,
            total_kwh,
            cost,
            config.cost.currency,
            duration / 3600,
            (duration % 3600) / 60,
            duration % 60,
        );

        std::thread::sleep(Duration::from_secs(1));
    }
}

fn cmd_ports() {
    match serialport::available_ports() {
        Ok(ports) => {
            if ports.is_empty() {
                println!("No serial ports found.");
            } else {
                println!("Available serial ports:");
                for p in &ports {
                    let info = match &p.port_type {
                        serialport::SerialPortType::UsbPort(usb) => {
                            format!(
                                "USB (VID:{:04x} PID:{:04x}{})",
                                usb.vid,
                                usb.pid,
                                usb.product
                                    .as_ref()
                                    .map(|s| format!(" - {}", s))
                                    .unwrap_or_default()
                            )
                        }
                        serialport::SerialPortType::BluetoothPort => "Bluetooth".to_string(),
                        serialport::SerialPortType::PciPort => "PCI".to_string(),
                        serialport::SerialPortType::Unknown => "Unknown".to_string(),
                    };
                    println!("  {} ({})", p.port_name, info);
                }
            }
        }
        Err(e) => {
            eprintln!("Error listing ports: {}", e);
            std::process::exit(1);
        }
    }
}

fn cmd_fan(config: &Config, action: FanAction) {
    let monitor = create_monitor(config);
    let handle = match monitor.start() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    wait_for_monitor_ready(&handle, Duration::from_secs(2));

    let result = match action {
        FanAction::Set { pwm } => handle
            .set_fan_pwm(pwm)
            .map(|_| format!("Fan PWM set to {} (风扇占空比已设置为 {})", pwm, pwm)),
        FanAction::Curve { json } => parse_curve_points(&json).and_then(|points| {
            let point_count = points.len();
            handle
                .set_fan_curve(points)
                .map(|_| format!("Fan curve applied (已应用风扇曲线), points={point_count}"))
        }),
        FanAction::Mode { mode } => mode.parse::<FanMode>().and_then(|parsed| {
            handle.set_fan_mode(parsed).map(|_| {
                format!(
                    "Fan mode set to {} (风扇模式已设置为 {})",
                    parsed,
                    parsed.label()
                )
            })
        }),
    };

    match result {
        Ok(message) => {
            std::thread::sleep(Duration::from_millis(500));
            let snap = handle.latest();
            println!("{message}");
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "device": {
                        "model": snap.device.model,
                        "serial": snap.device.serial,
                    },
                    "fan": {
                        "rpm": snap.fan.rpm,
                        "raw_mode_byte": snap.fan.pwm,
                    },
                    "meta": {
                        "connected": snap.meta.connected,
                        "packet_count": snap.meta.packet_count,
                        "data_age_s": snap.meta.data_age_s,
                    }
                }))
                .unwrap()
            );
        }
        Err(error) => {
            eprintln!("Fan command failed / 风扇命令失败: {}", error);
            handle.stop();
            std::process::exit(1);
        }
    }

    handle.stop();
}

fn wait_for_monitor_ready(handle: &wattson::serial::PsuHandle, timeout: Duration) {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let snap = handle.latest();
        if snap.meta.connected || snap.meta.packet_count > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn parse_curve_points(json: &str) -> wattson::Result<Vec<(u8, u8)>> {
    let parsed = serde_json::from_str::<Vec<[u8; 2]>>(json).map_err(|error| {
        wattson::WattsonError::Protocol {
            message: format!("invalid curve json: {error} / 曲线 JSON 无效: {error}"),
        }
    })?;

    Ok(parsed.into_iter().map(|[temp, pwm]| (temp, pwm)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_args_keeps_none_so_main_can_default_to_gui() {
        let cli = Cli::try_parse_from(["wattson"]).expect("parse default launch");
        assert!(cli.command.is_none());
    }

    #[test]
    fn legacy_tui_command_still_parses() {
        let cli = Cli::try_parse_from(["wattson", "tui", "--refresh", "450"]).expect("parse tui");
        assert!(matches!(cli.command, Some(Commands::Tui { refresh: 450 })));
    }

    #[test]
    fn legacy_read_command_still_parses() {
        let cli = Cli::try_parse_from(["wattson", "read", "--duration", "60"]).expect("parse read");
        assert!(matches!(cli.command, Some(Commands::Read { duration: 60 })));
    }

    #[test]
    fn legacy_serve_command_still_parses() {
        let cli = Cli::try_parse_from(["wattson", "serve", "--port", "9000"]).expect("parse serve");
        assert!(matches!(
            cli.command,
            Some(Commands::Serve { port: Some(9000) })
        ));
    }

    #[test]
    fn fan_set_command_parses() {
        let cli = Cli::try_parse_from(["wattson", "fan", "set", "55"]).expect("parse fan set");
        assert!(matches!(
            cli.command,
            Some(Commands::Fan {
                action: FanAction::Set { pwm: 55 }
            })
        ));
    }

    #[test]
    fn fan_curve_command_parses() {
        let cli = Cli::try_parse_from(["wattson", "fan", "curve", "[[30,40],[50,55],[70,75]]"])
            .expect("parse fan curve");
        assert!(matches!(
            cli.command,
            Some(Commands::Fan {
                action: FanAction::Curve { .. }
            })
        ));
    }

    #[test]
    fn fan_mode_command_parses() {
        let cli =
            Cli::try_parse_from(["wattson", "fan", "mode", "custom"]).expect("parse fan mode");
        assert!(matches!(
            cli.command,
            Some(Commands::Fan {
                action: FanAction::Mode { mode }
            }) if mode == "custom"
        ));
    }
}
