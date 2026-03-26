# Wattson

Universal digital PSU monitoring library — read real-time power consumption from your computer via serial protocols.

## What is this?

Wattson is a Rust library for communicating with digital power supply units (PSUs) that expose telemetry data over USB serial (UART). It parses vendor-specific protocols and provides a unified API for reading:

- **AC input power** (real-time wall power consumption)
- **DC output voltages** (+12V, +5V, +3.3V, +5VSB)
- **DC output currents** (estimated)
- **PSU temperature** (multiple sensors)
- **Fan speed** (RPM)
- **Device info** (model, serial number)

## Supported Hardware

| Brand | Series | Protocol | Status |
|-------|--------|----------|--------|
| Segotep (鑫谷) | DM series (DM-850G, DM-1000G, ...) | CH340 UART 115200-8N1 | ✅ Tested |
| Segotep (鑫谷) | KE series (KE-1300P) | CH340 UART 115200-8N1 | 🔧 Compatible (untested) |
| *Other brands* | *Contributions welcome* | — | — |

## Quick Start

```rust
use wattson::{PsuMonitor, Mode};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let monitor = PsuMonitor::new("COM4", Mode::Passive)?;
    let handle = monitor.start();

    loop {
        if let Some(snapshot) = handle.latest() {
            println!("AC Input: {:.1}W", snapshot.power.ac_input_w);
            println!("Efficiency: {:.1}%", snapshot.power.efficiency_pct);
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
```

## Examples

```bash
# Read PSU data once and output JSON
cargo run --example json_dump -- --port COM4

# Continuous monitoring
cargo run --example json_dump -- --port COM4 --watch
```

## Protocol References

- [YveMU/segotepk-psu-communication-protocol-driver](https://github.com/YveMU/segotepk-psu-communication-protocol-driver) — DM series protocol reverse engineering
- [SaltyFishOnTheSea/Segotep-PSU-Toolbox](https://github.com/SaltyFishOnTheSea/Segotep-PSU-Toolbox) — KE-1300P protocol analysis

## License

[CCOPL-1.0](LICENSE) — Contributors' Collective Ownership Public License

Copyright (c) 2026 ExoMind Collective
