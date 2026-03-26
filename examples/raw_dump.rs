//! Raw packet hex dump tool for protocol debugging
//! Usage: cargo run --example raw_dump -- COM4

use std::io::Read;
use std::time::{Duration, Instant};

fn main() {
    let port_name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "COM4".to_string());
    let duration = std::env::args()
        .nth(2)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30);

    eprintln!("Raw packet dump from {} for {}s...", port_name, duration);
    eprintln!("Send QUERY_CMD first to trigger broadcasts\n");

    let mut serial = serialport::new(&port_name, 115200)
        .timeout(Duration::from_secs(2))
        .open()
        .expect("Failed to open serial port");

    // Send query command to trigger data
    let query_cmd: [u8; 6] = [0x55, 0x7E, 0x02, 0x04, 0x06, 0xAE];
    serial.write_all(&query_cmd).expect("Failed to send query");

    let mut buf = Vec::with_capacity(4096);
    let mut read_buf = [0u8; 512];
    let start = Instant::now();
    let mut pkt_count = 0u32;

    while start.elapsed() < Duration::from_secs(duration) {
        match serial.read(&mut read_buf) {
            Ok(n) if n > 0 => buf.extend_from_slice(&read_buf[..n]),
            Ok(_) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                eprintln!("Read error: {}", e);
                break;
            }
        }

        // Find and dump all complete frames
        while let Some((payload, consumed, raw_frame)) = find_frame_raw(&buf) {
            pkt_count += 1;
            dump_frame(pkt_count, &payload, &raw_frame);
            buf = buf[consumed..].to_vec();
        }

        // Re-send query periodically
        if start.elapsed().as_secs() % 5 == 0 && pkt_count > 0 {
            let _ = serial.write_all(&query_cmd);
        }
    }

    eprintln!("\nDone. {} packets captured in {}s.", pkt_count, duration);
}

fn find_frame_raw(buf: &[u8]) -> Option<(Vec<u8>, usize, Vec<u8>)> {
    let mut i = 0;
    while i < buf.len().saturating_sub(4) {
        if buf[i] == 0x55 && buf[i + 1] == 0x7E {
            let pkt_len = buf[i + 2] as usize;
            if !(4..=200).contains(&pkt_len) {
                i += 1;
                continue;
            }
            let frame_end = i + 3 + pkt_len;
            if frame_end > buf.len() {
                return None;
            }
            let payload = buf[i + 3..i + 3 + pkt_len - 3].to_vec();
            let raw = buf[i..frame_end].to_vec();
            return Some((payload, frame_end, raw));
        }
        i += 1;
    }
    None
}

fn dump_frame(idx: u32, payload: &[u8], raw: &[u8]) {
    if payload.is_empty() {
        return;
    }

    let pkt_type = payload[0];
    let raw_hex: String = raw
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(" ");

    match pkt_type {
        0x02 => {
            // Electrical parameters (little-endian uint16)
            println!(
                "--- PKT #{:04} | 0x02 Electrical | len={} ---",
                idx,
                payload.len()
            );
            println!("  RAW: {}", raw_hex);
            if payload.len() >= 27 {
                let u16_le =
                    |off: usize| -> u16 { u16::from_le_bytes([payload[off], payload[off + 1]]) };
                let raw_vals: Vec<u16> = (0..13).map(|i| u16_le(1 + i * 2)).collect();
                println!("  raw u16 LE: {:?}", raw_vals);
                println!(
                    "  [0] 3.3V  = {} -> {:.3}V",
                    raw_vals[0],
                    raw_vals[0] as f64 / 1000.0
                );
                println!(
                    "  [1] 5V    = {} -> {:.3}V",
                    raw_vals[1],
                    raw_vals[1] as f64 / 1000.0
                );
                println!(
                    "  [2] 12V   = {} -> {:.3}V",
                    raw_vals[2],
                    raw_vals[2] as f64 / 1000.0
                );
                println!(
                    "  [3] 5VSB  = {} -> {:.3}V",
                    raw_vals[3],
                    raw_vals[3] as f64 / 1000.0
                );
                println!("  [4] I3.3V = {} (raw)", raw_vals[4]);
                println!("  [5] I5V   = {} (raw)", raw_vals[5]);
                println!("  [6] I12V  = {} (raw)", raw_vals[6]);
                println!(
                    "  [7] ACHz  = {} -> {:.1}Hz",
                    raw_vals[7],
                    raw_vals[7] as f64 / 10.0
                );
                println!("  [8]  unk  = {}", raw_vals[8]);
                println!("  [9]  unk  = {}", raw_vals[9]);
                println!("  [10] unk  = {}", raw_vals[10]);
                println!(
                    "  [11] ACV  = {} -> {:.1}V",
                    raw_vals[11],
                    raw_vals[11] as f64 / 10.0
                );
                println!(
                    "  [12] Fan  = {} -> {} RPM",
                    raw_vals[12],
                    raw_vals[12] as u32 * 30
                );
            }
        }
        0x04 => {
            // Extended status (big-endian uint16)
            println!(
                "--- PKT #{:04} | 0x04 Extended | len={} ---",
                idx,
                payload.len()
            );
            println!("  RAW: {}", raw_hex);
            let mode_byte = payload[1];
            let data = &payload[2..];
            let num = data.len() / 2;
            println!("  mode_byte = 0x{:02x} ({})", mode_byte, mode_byte);
            println!("  num_fields = {}", num);
            let u16_be = |i: usize| -> u16 { u16::from_be_bytes([data[i * 2], data[i * 2 + 1]]) };
            for i in 0..num {
                let raw_val = u16_be(i);
                let as_div10 = raw_val as f64 / 10.0;
                let as_div100 = raw_val as f64 / 100.0;
                let marker = match i {
                    0 => " <- temp_main (/10)",
                    6 => " <- ac_power_index=6 (/10)",
                    10 => " <- temp_air (/100)",
                    11 => " <- temp_air2 (/100)",
                    _ => "",
                };
                println!(
                    "  [{}] raw={:5} | /10={:7.1} | /100={:7.2}{}",
                    i, raw_val, as_div10, as_div100, marker
                );
            }
        }
        0x03 => {
            let text = String::from_utf8_lossy(&payload[1..])
                .trim_matches('\0')
                .trim()
                .to_string();
            println!("--- PKT #{:04} | 0x03 Model: \"{}\" ---", idx, text);
            println!("  RAW: {}", raw_hex);
        }
        0x05 => {
            let text = String::from_utf8_lossy(&payload[1..])
                .trim_matches('\0')
                .trim()
                .to_string();
            println!("--- PKT #{:04} | 0x05 Serial: \"{}\" ---", idx, text);
            println!("  RAW: {}", raw_hex);
        }
        _ => {
            println!(
                "--- PKT #{:04} | 0x{:02x} Unknown | len={} ---",
                idx,
                pkt_type,
                payload.len()
            );
            println!("  RAW: {}", raw_hex);
        }
    }
    println!();
}
