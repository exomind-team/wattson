<div align="center">

# ⚡ Wattson

**Universal Digital PSU Monitoring Library**

**Universal Digital PSU Monitoring Library — Energy Sensing Layer of the [ExoMind](https://github.com/exomind-team/exomind) Ecosystem**

通用数字电源监控库 — [ExoMind](https://github.com/exomind-team/exomind) 生态的能量感知层

[![License: CCOPL-1.0](https://img.shields.io/badge/License-CCOPL--1.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![CI](https://github.com/exomind-team/wattson/actions/workflows/ci.yml/badge.svg)](https://github.com/exomind-team/wattson/actions)

</div>

---

## 📊 Real-time Monitoring / 实时监控

<div align="center">

<img src="assets/demo_tui.png" alt="Wattson TUI Dashboard — DM-1000GD Real-time Monitoring" width="600">

*TUI dashboard: power trend chart, DC rails, thermal & fan, cost tracking*

</div>

| Metric | Min | Max | Avg |
|--------|-----|-----|-----|
| AC Input Power | 133W | 236W | **162W** |
| DC Output Power | 117W | 193W | 137W |
| Efficiency | 82% | 91% | 86% |

---

## ✨ Features / 功能特点

- **🔌 Real-time power monitoring** — AC input, DC output, conversion efficiency
  实时功率监控 — AC 输入、DC 输出、转换效率
- **📊 Chart generation** — Power/efficiency/temperature curves as PNG
  图表生成 — 功率/效率/温度曲线
- **💰 Electricity cost tracking** — Configurable price per kWh
  电费追踪 — 可配置电价
- **🖥️ Three modes** — CLI (one-shot), TUI (dashboard), API (HTTP server)
  三种模式 — 命令行、终端面板、HTTP API
- **⚙️ Config file** — TOML configuration with CLI management
  配置文件 — TOML 格式，CLI 可管理
- **🔄 Passive & Active** — Listen for broadcasts or actively query the PSU
  被动监听 & 主动查询两种通信模式

---

## 🔧 Supported Hardware / 支持的硬件

| Brand 品牌 | Series 系列 | Interface 接口 | Status 状态 |
|------------|-------------|---------------|-------------|
| Segotep 鑫谷 | DM series (DM-850G, **DM-1000G**, ...) | CH340 UART 115200-8N1 | ✅ Tested 已验证 |
| Segotep 鑫谷 | KE series (KE-1300P) | CH340 UART 115200-8N1 | 🔧 Compatible 兼容 |
| *Others 其他* | *Contributions welcome 欢迎贡献* | — | — |

---

## 🚀 Quick Start / 快速开始

### Install / 安装

```bash
# From source
git clone https://github.com/exomind-team/wattson.git
cd wattson
cargo build --release
```

### Initialize / 初始化

```bash
# Generate config file
wattson config init

# Check available ports
wattson ports
```

### Usage / 使用

```bash
# One-shot read (JSON output)  一次读取
wattson read

# Read for 60 seconds  读取 60 秒
wattson read --duration 60

# Continuous JSON stream  持续输出
wattson watch

# Interactive TUI dashboard  交互式面板
wattson tui

# HTTP API server  API 服务器
wattson serve

# Generate chart  生成图表
wattson chart --last 60
```

### Configuration / 配置管理

```bash
# View config  查看配置
wattson config show

# Set serial port  设置串口
wattson config set port COM4

# Set electricity price (CNY/kWh)  设置电价
wattson config set price 0.56

# Set communication mode  设置通信模式
wattson config set mode passive
```

### API Endpoints / API 接口

When running `wattson serve`:

| Endpoint | Description |
|----------|-------------|
| `GET /api/status` | Full PSU snapshot 完整快照 |
| `GET /api/power` | Power data only (AC/DC/efficiency) 功率数据 |
| `GET /api/voltage` | DC voltage readings 电压 |
| `GET /api/temperature` | Temperature sensors 温度 |
| `GET /api/device` | Device info 设备信息 |
| `GET /api/cost` | Electricity cost 电费 |
| `GET /health` | Health check 健康检查 |

---

## 📐 Configuration File / 配置文件

`wattson.toml`:

```toml
[serial]
port = "COM4"          # Serial port 串口
baud = 115200          # Baud rate 波特率
mode = "passive"       # passive | active 被动|主动

[device]
profile = "segotep_dm" # Device calibration profile 设备校准

[cost]
price_per_kwh = 0.56   # Electricity price 电价 (CNY/kWh)
currency = "CNY"       # Currency 货币

[chart]
output_dir = "./charts"
watermark = "Wattson | exomind-team/wattson"

[api]
port = 8066            # HTTP API port
```

---

## 🔬 Protocol Details / 协议细节

Wattson communicates with digital PSUs via USB serial (CH340/CH341 UART). The protocol uses a custom binary frame format:

```
[0x55][0x7E][LEN][PAYLOAD...][CHECKSUM_HI][CHECKSUM_LO][0xAE]
```

| Packet | Content | Byte Order |
|--------|---------|------------|
| `0x02` | Voltages, currents, AC input, fan RPM | Little-endian |
| `0x03` | Device model string | ASCII |
| `0x04` | Temperature, fan mode, AC power | Big-endian |
| `0x05` | Serial number | ASCII |

---

## 🙏 Acknowledgments / 致谢

Protocol reverse engineering based on:
- [YveMU/segotepk-psu-communication-protocol-driver](https://github.com/YveMU/segotepk-psu-communication-protocol-driver) — DM series protocol
- [SaltyFishOnTheSea/Segotep-PSU-Toolbox](https://github.com/SaltyFishOnTheSea/Segotep-PSU-Toolbox) — KE-1300P protocol

---

## 📄 License / 许可证

[CCOPL-1.0](LICENSE) — Contributors' Collective Ownership Public License

Copyright (c) 2026 ExoMind Collective

---

<div align="center">

**Part of the [ExoMind](https://github.com/exomind-team/exomind) ecosystem**

[ExoMind](https://github.com/exomind-team/exomind) · [ExoMind Cell](https://github.com/exomind-team/exomind-cell) · [CCOPL License](https://github.com/exomind-team/ccopl)

</div>
