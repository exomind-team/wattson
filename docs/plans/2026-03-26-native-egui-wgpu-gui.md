# Native egui + wgpu GUI Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Upgrade Wattson into a GUI-first native PSU monitor built on `egui` + `wgpu`, while preserving all existing CLI/TUI/API/chart behavior.

**Architecture:** Keep the existing serial/data/config/history core, add a testable app-state layer plus a demo data source, and build the desktop GUI with `eframe` using the `wgpu` renderer. Persist GUI settings separately so desktop controls survive restarts without breaking CLI usage.

**Tech Stack:** Rust, `eframe`, `egui`, `egui_plot`, `wgpu` renderer via `eframe`, `serde`, existing `clap`/`serialport`/`axum`/`ratatui`

---

### Task 1: Add GUI dependencies and the persistent GUI settings model

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/config.rs`
- Create: `src/gui_settings.rs`
- Modify: `src/lib.rs`
- Test: `src/gui_settings.rs`

**Step 1: Write the failing test**

Write tests for:
- default GUI settings values
- round-trip persistence of theme, chart time window, refresh rate, and visibility toggles

**Step 2: Run test to verify it fails**

Run: `cargo test gui_settings`
Expected: FAIL because `gui_settings` module and tests do not exist yet.

**Step 3: Write minimal implementation**

- Add `eframe`, `egui`, `egui_plot`
- Add a `GuiSettings` struct with serde support
- Save/load to a stable file path near config

**Step 4: Run test to verify it passes**

Run: `cargo test gui_settings`
Expected: PASS

**Step 5: Commit**

```bash
git add Cargo.toml src/config.rs src/gui_settings.rs src/lib.rs
git commit -m "feat: add persisted gui settings"
```

### Task 2: Build a testable runtime state and demo data source

**Files:**
- Create: `src/runtime.rs`
- Modify: `src/data.rs`
- Modify: `src/history.rs`
- Modify: `src/lib.rs`
- Test: `src/runtime.rs`

**Step 1: Write the failing test**

Write tests for:
- pushing snapshots into a ring buffer
- trimming history to a requested time window
- computing derived stats for AC/DC/cost
- deterministic demo waveform generation

**Step 2: Run test to verify it fails**

Run: `cargo test runtime`
Expected: FAIL because runtime state does not exist.

**Step 3: Write minimal implementation**

- Add a runtime state object that is UI-friendly and independent from TUI
- Introduce a `SnapshotSource` abstraction / 数据源抽象 for real vs demo data
- Add deterministic demo samples

**Step 4: Run test to verify it passes**

Run: `cargo test runtime`
Expected: PASS

**Step 5: Commit**

```bash
git add src/runtime.rs src/data.rs src/history.rs src/lib.rs
git commit -m "feat: add gui runtime state and demo source"
```

### Task 3: Add the `eframe` GUI application with theme and chart controls

**Files:**
- Create: `src/gui.rs`
- Modify: `src/main.rs`
- Modify: `src/lib.rs`
- Test: `src/gui.rs`

**Step 1: Write the failing test**

Write tests for:
- theme preference changes update the app state
- chart visibility toggles affect rendered series selection
- refresh/poll controls clamp correctly

**Step 2: Run test to verify it fails**

Run: `cargo test gui::tests`
Expected: FAIL because GUI app state does not exist.

**Step 3: Write minimal implementation**

- Add `gui` subcommand
- Optionally allow no subcommand to default to GUI
- Build top/side/bottom panels and center plot
- Use `egui::ThemePreference`
- Force `NativeOptions.renderer = Renderer::Wgpu`

**Step 4: Run test to verify it passes**

Run: `cargo test gui::tests`
Expected: PASS

**Step 5: Commit**

```bash
git add src/gui.rs src/main.rs src/lib.rs
git commit -m "feat: add egui wgpu desktop dashboard"
```

### Task 4: Add screenshot/export and end-to-end demo verification

**Files:**
- Modify: `src/main.rs`
- Modify: `src/gui.rs`
- Create: `tests/gui_e2e.rs`
- Test: `tests/gui_e2e.rs`

**Step 1: Write the failing test**

Write an end-to-end test that:
- launches demo rendering path
- writes a screenshot PNG
- asserts file exists and has non-zero size

**Step 2: Run test to verify it fails**

Run: `cargo test --test gui_e2e -- --nocapture`
Expected: FAIL because screenshot path is not implemented.

**Step 3: Write minimal implementation**

- Add a non-interactive screenshot code path for demo mode
- Ensure it can run in CI/local verification without hardware

**Step 4: Run test to verify it passes**

Run: `cargo test --test gui_e2e -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/main.rs src/gui.rs tests/gui_e2e.rs
git commit -m "test: add gui screenshot e2e coverage"
```

### Task 5: Update docs and validate retained legacy flows

**Files:**
- Modify: `README.md`
- Create: `docs/screenshots/`
- Test: existing CLI/TUI/API smoke commands

**Step 1: Write the failing test**

Add or run smoke coverage that proves CLI parsing still supports:
- `read`
- `tui`
- `serve`
- `chart`
- `ports`

**Step 2: Run test to verify it fails**

Run: `cargo test cli`
Expected: FAIL if parser/default command behavior broke legacy subcommands.

**Step 3: Write minimal implementation**

- Update README with GUI mode and screenshot
- Adjust CLI parser so legacy commands remain intact

**Step 4: Run test to verify it passes**

Run: `cargo test cli`
Expected: PASS

**Step 5: Commit**

```bash
git add README.md docs/screenshots
git commit -m "docs: document native gui workflow"
```

### Task 6: Final verification, PR, and diary log

**Files:**
- Modify: `D:/project/ExoMind-Obsidian-HailayLin/2-个人状态与历史记录/日记/2026-03-26.md`

**Step 1: Run full verification**

Run:

```bash
cargo test
cargo test --test gui_e2e -- --nocapture
cargo run -- gui --demo --screenshot docs/screenshots/gui-demo.png
```

Expected:
- all tests pass
- screenshot generated

**Step 2: Review requirements line by line**

Check:
- GUI is `egui + wgpu`
- light/dark/system theme works
- legacy features preserved
- screenshot exists
- issue and PR are linked

**Step 3: Commit final docs/assets**

```bash
git add .
git commit -m "feat: ship native egui wgpu wattson dashboard"
```

**Step 4: Push and create PR**

```bash
git push -u origin feat/native-egui-wgpu-gui
gh pr create --fill --base master --head feat/native-egui-wgpu-gui
```

**Step 5: Append diary record**

- Write issue/branch/PR/screenshot links into the 2026-03-26 diary file
