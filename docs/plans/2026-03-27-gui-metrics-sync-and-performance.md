# GUI Metrics Sync And Performance Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Align GUI and TUI power/energy averages on one shared calculation path, fill the missing summary metrics, and expose UI refresh performance clearly without regressing live serial behavior.

**Architecture:** Extend the shared runtime accumulator so both GUI and TUI derive session and all-time metrics from the same source of truth. Keep rendering code thin: UI layers read precomputed metrics, while live serial ingestion and history persistence stay in their current modules.

**Tech Stack:** Rust, `eframe`/`egui`, existing `serialport` runtime, `ratatui`, `cargo test`, `cargo clippy`

---

### Task 1: Lock the shared metric definitions with tests

**Files:**
- Modify: `tests/runtime.rs`

**Step 1: Write the failing test**

- Add coverage for:
  - session AC average and session DC average
  - session duration and session energy
  - all-time average computed from `History + RuntimeState`

**Step 2: Run test to verify it fails**

Run: `cargo test runtime_ -- --nocapture`
Expected: FAIL because the new shared metrics are not implemented yet.

**Step 3: Write minimal implementation**

- Extend `src/runtime.rs` to accumulate AC/DC energy and expose shared metrics helpers.

**Step 4: Run test to verify it passes**

Run: `cargo test runtime_ -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add tests/runtime.rs src/runtime.rs
git commit -m "test: lock shared runtime metrics"
```

### Task 2: Move GUI and TUI to the shared metric path

**Files:**
- Modify: `src/gui.rs`
- Modify: `src/tui.rs`

**Step 1: Write the failing test**

- Add or extend GUI tests to verify the app exposes complete summary metrics and refresh stats labels through the shared runtime path where practical.

**Step 2: Run test to verify it fails**

Run: `cargo test gui_app -- --nocapture`
Expected: FAIL before the UI/model wiring is updated.

**Step 3: Write minimal implementation**

- Replace duplicated average/energy math with shared runtime metrics.
- Show complete summary content:
  - current AC/DC
  - session average AC/DC
  - session energy/cost/time
  - all-time energy/cost/average
- Surface actual/target refresh performance in the GUI.
- Update TUI labels so the displayed averages use the same definitions.

**Step 4: Run test to verify it passes**

Run: `cargo test gui_app -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/gui.rs src/tui.rs tests/gui_app.rs
git commit -m "feat: align dashboard metrics across gui and tui"
```

### Task 3: Full verification and evidence capture

**Files:**
- Modify: `README.md` (only if wording must match shipped behavior)
- Modify: `D:\project\ExoMind-Obsidian-HailayLin\2-个人状态与历史记录\日记\2026-03-27.md`

**Step 1: Run automated verification**

Run:
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`

Expected: all green

**Step 2: Run live-device verification**

Run:
- `cargo run -- info`
- `cargo run -- read --duration 3`
- `cargo run -- gui` for live inspection on `COM4`

Expected:
- real PSU metadata and telemetry
- GUI shows live source and synchronized metrics

**Step 3: Capture evidence**

- Update screenshots only if visuals changed materially.
- Append the branch / issue / PR / verification links to the daily log.

**Step 4: Commit**

```bash
git add docs/plans README.md
git commit -m "docs: record gui metrics sync verification"
```
