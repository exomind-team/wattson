//! Poll rate benchmark: test PSU response at different query intervals
//! Usage: cargo run --example poll_bench -- COM4

use std::io::Read;
use std::time::{Duration, Instant};

const QUERY_CMD: [u8; 6] = [0x55, 0x7E, 0x02, 0x04, 0x06, 0xAE];

fn find_frame(buf: &[u8]) -> Option<(u8, usize)> {
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
            let pkt_type = buf[i + 3];
            return Some((pkt_type, frame_end));
        }
        i += 1;
    }
    None
}

fn test_interval(port_name: &str, poll_ms: u64, duration_secs: u64) -> (u32, u32, u32, u32) {
    let mut serial = match serialport::new(port_name, 115200)
        .timeout(Duration::from_secs(2))
        .open()
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("  Cannot open {}: {}", port_name, e);
            return (0, 0, 0, 0);
        }
    };

    let _ = serial.write_all(&QUERY_CMD);

    let mut buf = Vec::with_capacity(4096);
    let mut read_buf = [0u8; 512];
    let mut last_query = Instant::now();
    let start = Instant::now();

    let mut count_02 = 0u32;
    let mut count_03 = 0u32;
    let mut count_04 = 0u32;
    let mut count_other = 0u32;

    let poll_interval = Duration::from_millis(poll_ms);

    while start.elapsed() < Duration::from_secs(duration_secs) {
        match serial.read(&mut read_buf) {
            Ok(n) if n > 0 => buf.extend_from_slice(&read_buf[..n]),
            Ok(_) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => break,
        }

        if last_query.elapsed() > poll_interval {
            let _ = serial.write_all(&QUERY_CMD);
            last_query = Instant::now();
        }

        while let Some((pkt_type, consumed)) = find_frame(&buf) {
            match pkt_type {
                0x02 => count_02 += 1,
                0x03 => count_03 += 1,
                0x04 => count_04 += 1,
                _ => count_other += 1,
            }
            buf = buf[consumed..].to_vec();
        }

        if buf.len() > 8192 {
            buf.drain(..buf.len() - 1024);
        }
    }

    (count_02, count_03, count_04, count_other)
}

fn main() {
    let port = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "COM4".to_string());
    let test_secs = 10;

    println!("Poll rate benchmark on {} ({}s each)\n", port, test_secs);
    println!(
        "{:>8} | {:>6} {:>6} {:>6} {:>6} | {:>5} {:>5}",
        "Poll ms", "0x02", "0x03", "0x04", "other", "02/s", "04/s"
    );
    println!("{}", "-".repeat(65));

    for &ms in &[100, 200, 300, 400, 500, 750, 1000, 2000] {
        let (c02, c03, c04, cother) = test_interval(&port, ms, test_secs);
        println!(
            "{:>6}ms | {:>6} {:>6} {:>6} {:>6} | {:>5.1} {:>5.1}",
            ms,
            c02,
            c03,
            c04,
            cother,
            c02 as f64 / test_secs as f64,
            c04 as f64 / test_secs as f64,
        );
    }
}
