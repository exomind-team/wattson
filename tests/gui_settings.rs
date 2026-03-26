use std::path::PathBuf;

use wattson::gui_settings::{ChartScaleMode, GuiSettings, ThemePreference};

fn temp_settings_path(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("wattson-{name}-{}.json", std::process::id()));
    path
}

#[test]
fn gui_settings_defaults_are_desktop_friendly() {
    let settings = GuiSettings::default();

    assert_eq!(settings.theme, ThemePreference::System);
    assert_eq!(settings.chart_window_s, 120);
    assert_eq!(settings.ui_refresh_ms, 16);
    assert_eq!(settings.poll_interval_ms, 300);
    assert!(settings.show_ac_input);
    assert!(settings.show_dc_output);
    assert_eq!(settings.chart_scale, ChartScaleMode::Auto);
}

#[test]
fn gui_settings_round_trip_persistence() {
    let path = temp_settings_path("roundtrip");
    let _ = std::fs::remove_file(&path);

    let settings = GuiSettings {
        theme: ThemePreference::Dark,
        chart_window_s: 300,
        ui_refresh_ms: 33,
        poll_interval_ms: 500,
        show_ac_input: true,
        show_dc_output: false,
        chart_scale: ChartScaleMode::ZeroBased,
        ..GuiSettings::default()
    };

    settings.save_to(&path).expect("save GUI settings");
    let loaded = GuiSettings::load_from(&path).expect("load GUI settings");

    assert_eq!(loaded.theme, ThemePreference::Dark);
    assert_eq!(loaded.chart_window_s, 300);
    assert_eq!(loaded.ui_refresh_ms, 33);
    assert_eq!(loaded.poll_interval_ms, 500);
    assert!(loaded.show_ac_input);
    assert!(!loaded.show_dc_output);
    assert_eq!(loaded.chart_scale, ChartScaleMode::ZeroBased);

    let _ = std::fs::remove_file(path);
}
