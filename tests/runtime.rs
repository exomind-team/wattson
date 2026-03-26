use chrono::{Duration, TimeZone, Utc};

use wattson::data::PsuSnapshot;
use wattson::runtime::{DemoGenerator, RuntimeState};

fn sample_snapshot(ac_input_w: f64, dc_output_w: f64) -> PsuSnapshot {
    let mut snapshot = PsuSnapshot::default();
    snapshot.power.ac_input_w = ac_input_w;
    snapshot.power.dc_output_est_w = dc_output_w;
    snapshot.power.efficiency_pct = if ac_input_w > 0.0 {
        dc_output_w / ac_input_w * 100.0
    } else {
        0.0
    };
    snapshot.meta.connected = true;
    snapshot
}

#[test]
fn runtime_trims_history_to_requested_window() {
    let mut state = RuntimeState::new(0.56, "CNY");
    let t0 = Utc.with_ymd_and_hms(2026, 3, 26, 12, 0, 0).unwrap();

    state.push_snapshot(t0, sample_snapshot(100.0, 90.0));
    state.push_snapshot(t0 + Duration::seconds(60), sample_snapshot(120.0, 100.0));
    state.push_snapshot(t0 + Duration::seconds(121), sample_snapshot(140.0, 110.0));

    let window = state.samples_in_window(120);

    assert_eq!(window.len(), 2);
    assert_eq!(window[0].snapshot.power.ac_input_w, 120.0);
    assert_eq!(window[1].snapshot.power.ac_input_w, 140.0);
}

#[test]
fn runtime_accumulates_energy_and_cost_from_samples() {
    let mut state = RuntimeState::new(0.56, "CNY");
    let t0 = Utc.with_ymd_and_hms(2026, 3, 26, 12, 0, 0).unwrap();

    state.push_snapshot(t0, sample_snapshot(100.0, 80.0));
    state.push_snapshot(t0 + Duration::minutes(30), sample_snapshot(100.0, 80.0));
    state.push_snapshot(t0 + Duration::minutes(60), sample_snapshot(100.0, 80.0));

    let stats = state.stats();

    assert!((stats.total_kwh - 0.1).abs() < 1e-6);
    assert!((stats.total_cost - 0.056).abs() < 1e-6);
    assert_eq!(stats.currency, "CNY");
    assert!((stats.average_ac_input_w - 100.0).abs() < 1e-6);
}

#[test]
fn demo_generator_is_deterministic_for_a_given_step() {
    let sample_a = DemoGenerator::sample_at(42);
    let sample_b = DemoGenerator::sample_at(42);
    let sample_c = DemoGenerator::sample_at(43);

    assert_eq!(sample_a.power.ac_input_w, sample_b.power.ac_input_w);
    assert_eq!(
        sample_a.power.dc_output_est_w,
        sample_b.power.dc_output_est_w
    );
    assert_ne!(sample_a.power.ac_input_w, sample_c.power.ac_input_w);
    assert!(sample_a.meta.connected);
}
