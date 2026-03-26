use std::time::Duration;
use wattson::{PsuMonitor, Mode, DeviceProfile};

fn main() {
    let mut port = "COM4".to_string();
    let mut watch = false;

    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" | "-p" => {
                i += 1;
                if i < args.len() { port = args[i].clone(); }
            }
            "--watch" | "-w" => watch = true,
            "--help" | "-h" => {
                println!("Usage: json_dump [--port COM4] [--watch]");
                println!("  --port, -p   Serial port (default: COM4)");
                println!("  --watch, -w  Continuous output (1 JSON per second)");
                return;
            }
            _ => {}
        }
        i += 1;
    }

    eprintln!("Connecting to {} ...", port);

    let monitor = PsuMonitor::new(&port, Mode::Passive)
        .with_profile(DeviceProfile::SEGOTEP_DM);

    let handle = match monitor.start() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Failed to start: {}", e);
            std::process::exit(1);
        }
    };

    // Wait for first data
    for _ in 0..50 {
        if handle.latest().meta.packet_count > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    if watch {
        eprintln!("Streaming (Ctrl+C to stop)...");
        loop {
            let snap = handle.latest();
            println!("{}", serde_json::to_string(&snap).unwrap());
            std::thread::sleep(Duration::from_secs(1));
        }
    } else {
        // Wait a bit more for all packet types
        std::thread::sleep(Duration::from_secs(3));
        let snap = handle.latest();
        println!("{}", serde_json::to_string_pretty(&snap).unwrap());
        handle.stop();
    }
}
