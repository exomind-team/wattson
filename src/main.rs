use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Parser, Subcommand};
use wattson::config::Config;
use wattson::data::DeviceProfile;
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
    command: Commands,
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
        #[arg(long, default_value_t = 500)]
        refresh: u64,
    },
    /// Start HTTP API server
    Serve {
        /// Port to listen on (overrides config)
        #[arg(long)]
        port: Option<u16>,
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

    match cli.command {
        Commands::Read { duration } => cmd_read(&config, duration),
        Commands::Watch => cmd_watch(&config),
        Commands::Config { action } => cmd_config(&mut config, action),
        Commands::Tui { refresh } => cmd_tui(&config, refresh),
        Commands::Serve { port } => cmd_serve(&mut config, port),
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
