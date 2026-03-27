# DM-1000G Write Protocol And Fan Control Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Reverse-engineer (逆向分析) the Segotep/Xingu DM-1000G serial write protocol for fan control, then implement safe fan-speed and fan-curve write support in Wattson without regressing the existing read path.

**Architecture:** Treat this as two phases. Phase A captures and validates the vendor software's serial writes on Windows, then documents the write frame format in `docs/protocol.md`. Phase B adds write-frame serialization, a single-owner serial command path inside `src/serial.rs`, and controlled API/CLI surfaces that reuse the same live COM session instead of opening the port a second time.

**Tech Stack:** Rust, `serialport`, `axum`, `clap`, Windows 11, PowerShell, official vendor PSU tool, Free Serial Port Monitor or `com0com`/`hub4com`, real hardware on `COM4`

---

## Decision Summary（方案定稿）

- **Recommended sniffing path（推荐抓包路径）:** use a real serial sniffer first. If the sniffer cannot attach to `COM4`, fall back to `com0com` + `hub4com` and make the vendor tool talk to a virtual port that is forwarded to the real PSU port.
- **Recommended runtime design（推荐运行时设计）:** keep the serial port owned by exactly one background worker thread. API/CLI/TUI/GUI submit write requests through a command channel instead of opening the COM port separately.
- **Do not start with code（不要先写代码）:** Task A is a hard prerequisite. Without captured write frames, any write implementation is guesswork and unsafe for the PSU.

## Current Code Baseline（当前代码基线）

- `src/protocol.rs`
  - Has read-only protocol parsing and the active query frame `QUERY_CMD`.
  - No public frame builder / serializer for outbound write commands.
- `src/serial.rs`
  - The background thread opens the port and periodically sends `QUERY_CMD`.
  - `PsuHandle` does not expose any write path, and no shared command queue exists yet.
- `src/api.rs`
  - Only `GET` routes exist, and CORS currently allows only `GET`.
- `src/main.rs`
  - `clap` command tree is already structured and can cleanly accept a `fan` subcommand family.
- `docs/protocol.md`
  - Does not exist yet, so the write protocol documentation must be created from scratch.

### Task 1: Prepare isolated workspace and hardware baseline

**Files:**
- Create: `docs/protocol.md`
- Modify: `docs/plans/2026-03-27-dm1000g-write-protocol-and-fan-control.md`

**Step 1: Verify the live device and current read path**

Run:

```powershell
cargo run -- ports
cargo run -- info
cargo run -- read --duration 3
Get-CimInstance Win32_SerialPort | Select-Object DeviceID, Name, Description
```

Expected:
- `COM4` is visible and stable.
- Existing read protocol still works before any reverse-engineering work starts.

**Step 2: Isolate this work from the current `master`**

Run:

```powershell
git status --short --branch
git switch -c feat/dm1000g-fan-write
```

Expected:
- New work happens on a dedicated branch instead of the current `master...origin/master [ahead 7]` branch state.

**Step 3: Create the protocol note shell**

- Create `docs/protocol.md` with sections for:
  - frame layout（帧结构）
  - read commands（读取命令）
  - write commands（写入命令）
  - checksum validation（校验算法）
  - capture evidence（抓包证据）

### Task 2: Build the Windows sniffing setup

**Files:**
- Modify: `docs/protocol.md`

**Step 1: Try the direct sniffer path first**

- Install and launch Free Serial Port Monitor (or equivalent that can attach to a live Windows COM port).
- Attach it to `COM4`.

Expected:
- If the tool can observe both RX/TX on `COM4`, keep this path and skip the virtual-port detour.

**Step 2: If direct attach fails, build a virtual bridge**

- Install `com0com` and `hub4com`.
- Create a virtual pair such as `COM11 <-> COM12`.
- Point the vendor software to `COM11`.
- Forward `COM11` traffic to real `COM4`, while logging both directions.

Recommended command pattern (adjust paths to the actual install location):

```powershell
hub4com --baud=115200 --octs=off --ito=50 --route=all COM11 COM4
```

Expected:
- Vendor tool believes it is talking to a normal serial PSU port.
- You can capture all outbound writes before they hit `COM4`.

**Step 3: Confirm exclusivity rules**

- Do **not** run `wattson` against `COM4` while the vendor software owns that port directly.
- If using the virtual bridge, only one process should own each endpoint at a time.

### Task 3: Capture a complete fan-control operation matrix

**Files:**
- Modify: `docs/protocol.md`

**Step 1: Capture a baseline idle session**

- Open the vendor tool and do nothing for 20 to 30 seconds.
- Save the raw hex stream and mark it as `idle`.

Expected:
- You can separate recurring telemetry polling from actual user-triggered write frames.

**Step 2: Capture one-variable-at-a-time changes**

- Perform these actions one by one, with at least 5 seconds between each:
  - set fixed PWM to `30`
  - set fixed PWM to `50`
  - set fixed PWM to `70`
  - set fixed PWM to `100`
  - switch fan mode from auto to manual
  - switch fan mode from manual to auto
  - edit exactly one curve point
  - edit two adjacent curve points
  - apply / save the curve

Expected:
- Each operator action maps to one or a small cluster of new outbound frames.

**Step 3: Repeat each capture at least twice**

Expected:
- Stable bytes can be distinguished from timestamps, counters, or session-specific noise.

**Step 4: Record the evidence in `docs/protocol.md`**

- For each action, write down:
  - timestamp
  - UI action（界面动作）
  - raw TX bytes
  - immediate RX response
  - observed PSU behavior（风扇转速 / 模式变化）

### Task 4: Decode the write frame format and lock the protocol document

**Files:**
- Modify: `docs/protocol.md`

**Step 1: Derive the common frame skeleton**

- Compare captured writes against the known read frame header `55 7E`.
- Identify:
  - fixed header bytes
  - length byte semantics
  - command / packet type byte
  - payload region
  - checksum byte
  - footer byte

Expected:
- A candidate frame template emerges, for example:
  - `header | len | cmd | payload | checksum | footer`

**Step 2: Reverse the checksum**

- Test the captured frames against likely checksum families:
  - byte sum modulo 256
  - two's complement of sum
  - XOR
  - length-inclusive vs payload-only sum

Expected:
- One algorithm matches every captured write frame, not just a subset.

**Step 3: Infer payload semantics**

- For fixed PWM commands, map which byte tracks the requested duty cycle.
- For curve writes, determine whether the payload is:
  - a full curve table in one frame
  - per-point updates across multiple frames
  - a staged write plus final commit command

**Step 4: Validate on hardware**

- Reproduce one captured write frame manually through a serial sender.
- Confirm the PSU changes state exactly as the vendor tool did.

Expected:
- At least one fixed PWM command and one curve-related command are confirmed on real hardware.

**Step 5: Finalize `docs/protocol.md`**

- Document:
  - command names（命令名）
  - full byte layout（完整字节布局）
  - checksum formula（校验公式）
  - worked examples（实例）
  - unknown fields / open questions（未解字段）

### Task 5: Add protocol serialization tests before implementation

**Files:**
- Modify: `src/protocol.rs`
- Create: `tests/protocol_write.rs`

**Step 1: Write failing protocol tests**

- Add tests for:
  - `build_query_frame()` reproduces the current `QUERY_CMD`
  - `build_fan_pwm_frame(30)` matches the captured manual frame
  - `build_fan_curve_frame(...)` matches one captured curve example
  - checksum helper rejects malformed examples

**Step 2: Run tests to verify they fail**

Run:

```powershell
cargo test protocol_write -- --nocapture
```

Expected:
- FAIL because the write serializers do not exist yet.

**Step 3: Implement minimal frame-building helpers**

- In `src/protocol.rs`, add:
  - `build_frame(...)`
  - `checksum(...)`
  - `build_fan_pwm_frame(value: u8)`
  - `build_fan_curve_frame(points: &[(u8, u8)])`

**Step 4: Run tests to verify they pass**

Run:

```powershell
cargo test protocol_write -- --nocapture
```

Expected:
- PASS with captured bytes matching exactly.

### Task 6: Refactor serial runtime to support safe writes

**Files:**
- Modify: `src/serial.rs`
- Modify: `src/error.rs`
- Create: `tests/serial_commands.rs`

**Step 1: Write failing runtime tests**

- Add tests around a command-dispatch abstraction that prove:
  - only one worker owns the serial port
  - API/CLI callers can enqueue a write request
  - a request can wait for ACK / timeout
  - polling resumes after a write

**Step 2: Run tests to verify they fail**

Run:

```powershell
cargo test serial_commands -- --nocapture
```

Expected:
- FAIL because `PsuHandle` has no command path yet.

**Step 3: Implement the single-owner command channel**

- Keep the actual COM port inside the reader worker thread.
- Add a `SerialCommand` enum, for example:
  - `SetFanPwm(u8)`
  - `SetFanCurve(Vec<(u8, u8)>)`
  - `Raw(Vec<u8>)` for bring-up/debug only
- Add a request/response channel so callers can receive `Result<()>`.
- Expose methods on `PsuHandle`:
  - `set_fan_pwm(value: u8)`
  - `set_fan_curve(points: Vec<(u8, u8)>)`

**Step 4: Handle timing safely**

- Pause periodic `QUERY_CMD` emission while a write command is in flight.
- Optionally drain / parse immediate ACK frames before resuming polling.
- Add bounded timeout errors in `src/error.rs`.

**Step 5: Run tests to verify they pass**

Run:

```powershell
cargo test serial_commands -- --nocapture
```

Expected:
- PASS with no second-port-open behavior and no deadlock.

### Task 7: Expose the write path through HTTP API

**Files:**
- Modify: `src/api.rs`
- Modify: `Cargo.toml`
- Create: `tests/api_fan.rs`

**Step 1: Write failing API tests**

- Add request tests for:
  - `POST /api/fan/speed` with `{ "pwm": 50 }`
  - `POST /api/fan/curve` with a valid point list
  - bad payload returns `400`
  - device write failure returns `502` or `500`

**Step 2: Run tests to verify they fail**

Run:

```powershell
cargo test api_fan -- --nocapture
```

Expected:
- FAIL because the routes and request DTOs do not exist.

**Step 3: Implement minimal API support**

- In `src/api.rs`:
  - add JSON request structs
  - enable `POST` in CORS
  - add routes:
    - `POST /api/fan/speed`
    - `POST /api/fan/curve`
- If needed for router testing, add a dev dependency such as `tower = "0.5"` in `Cargo.toml`.

**Step 4: Run tests to verify they pass**

Run:

```powershell
cargo test api_fan -- --nocapture
```

Expected:
- PASS with valid request validation and mapped error codes.

### Task 8: Expose the write path through CLI

**Files:**
- Modify: `src/main.rs`

**Step 1: Write failing CLI parse tests**

- Extend the existing CLI parser tests with:
  - `wattson fan set 55`
  - `wattson fan curve '[ [30,40], [50,55], [70,75] ]'`

**Step 2: Run tests to verify they fail**

Run:

```powershell
cargo test main::tests -- --nocapture
```

Expected:
- FAIL because the `fan` subcommand tree does not exist.

**Step 3: Implement minimal CLI wiring**

- Add:
  - `wattson fan set <pwm>`
  - `wattson fan curve <json>`
- Reuse the same `PsuHandle` write methods rather than duplicating serial logic.
- Print clear stdout/stderr messages in both English and Chinese keywords where useful, for example:
  - `Fan PWM set to 55 (风扇占空比已设置为 55)`

**Step 4: Run tests to verify they pass**

Run:

```powershell
cargo test main::tests -- --nocapture
```

Expected:
- PASS with the new subcommands parsed correctly.

### Task 9: End-to-end verification on the real PSU

**Files:**
- Modify: `docs/protocol.md`
- Modify: `README.md`

**Step 1: Run the full automated suite**

Run:

```powershell
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected:
- All green.

**Step 2: Verify fixed-speed control on `COM4`**

Run:

```powershell
cargo run -- fan set 30
cargo run -- fan set 60
cargo run -- fan set 100
```

Expected:
- Reported `fan_rpm` changes in the expected direction.
- No reader-thread disconnects or COM-port busy errors.

**Step 3: Verify API control**

Run:

```powershell
cargo run -- serve --port 9000
Invoke-RestMethod -Method Post -Uri http://127.0.0.1:9000/api/fan/speed -ContentType 'application/json' -Body '{\"pwm\":50}'
Invoke-RestMethod -Method Post -Uri http://127.0.0.1:9000/api/fan/curve -ContentType 'application/json' -Body '{\"points\":[[30,40],[50,55],[70,75],[85,100]]}'
```

Expected:
- HTTP returns success JSON.
- Live RPM / mode telemetry reflects the new control state.

**Step 4: Update docs for shipped behavior**

- In `README.md`, add the new CLI and API examples.
- In `docs/protocol.md`, mark which commands were hardware-verified and which remain inferred.

## Risks And Stop Conditions（风险与停止条件）

- If the vendor tool talks over a USB HID endpoint instead of the exposed CH340/CH341 serial channel, stop and re-scope Task A before writing code.
- If the PSU requires a session unlock / handshake before accepting writes, document that handshake first; do not hard-code fan commands in isolation.
- If curve writes are stateful multi-step transactions, do not ship `set_fan_curve` until the full commit/rollback behavior is understood on hardware.
- Do not add config-file persistence for fan control in the first pass unless the hardware behavior is proven safe after reboot and replug.

## Suggested Execution Order（建议执行顺序）

1. Complete Task 1 through Task 4 and get at least one hardware-verified write frame.
2. Only then start Task 5 through Task 8.
3. Keep Task 9 for the same branch before any merge / PR.
