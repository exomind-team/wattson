use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Color32};
use wattson::gui::{format_serial_for_display, GuiApp};
use wattson::gui_settings::ThemePreference;

fn temp_screenshot_path(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_nanos();
    std::env::temp_dir()
        .join(format!("wattson-{name}-{unique}"))
        .join("native-export.png")
}

#[test]
fn gui_app_updates_theme_and_series_visibility() {
    let mut app = GuiApp::demo();

    app.set_theme(ThemePreference::Dark);
    app.set_series_visibility(true, false);

    assert_eq!(app.settings().theme, ThemePreference::Dark);
    assert_eq!(app.visible_series_count(), 1);
}

#[test]
fn gui_app_clamps_refresh_and_poll_controls() {
    let mut app = GuiApp::demo();

    app.set_ui_refresh_ms(1);
    app.set_poll_interval_ms(50);

    assert_eq!(app.settings().ui_refresh_ms, 16);
    assert_eq!(app.settings().poll_interval_ms, 200);

    app.set_ui_refresh_ms(5_000);
    app.set_poll_interval_ms(9_000);

    assert_eq!(app.settings().ui_refresh_ms, 1_000);
    assert_eq!(app.settings().poll_interval_ms, 5_000);
}

#[test]
fn gui_app_exposes_complete_summary_metrics() {
    let app = GuiApp::demo();
    let metrics = app.summary_metrics().expect("summary metrics");

    assert!(metrics.session_kwh > 0.0);
    assert!(metrics.session_avg_ac_input_w > 0.0);
    assert!(metrics.session_avg_dc_output_w > 0.0);
    assert!(metrics.all_time_kwh >= metrics.session_kwh);
    assert_eq!(metrics.currency, "CNY");
}

#[test]
fn gui_app_reports_refresh_performance_targets() {
    let mut app = GuiApp::demo();
    app.set_ui_refresh_ms(20);

    let perf = app.performance_stats();

    assert!((perf.target_fps - 50.0).abs() < 0.1);
    assert!(perf.frame_time_ms >= 0.0);
}

#[test]
fn gui_app_updates_persisted_window_dimensions_from_viewport_changes() {
    let mut app = GuiApp::demo();

    app.sync_window_size(1600.0, 960.0);

    assert_eq!(app.settings().window_width, 1600);
    assert_eq!(app.settings().window_height, 960);
}

#[test]
fn gui_demo_prefills_enough_history_for_the_default_chart_window() {
    let app = GuiApp::demo();
    let metrics = app.summary_metrics().expect("summary metrics");

    assert!(metrics.session_duration_s >= app.settings().chart_window_s as f64 - 1.0);
}

#[test]
fn serial_number_is_masked_by_default_for_privacy() {
    let app = GuiApp::demo();

    assert!(!app.settings().show_full_serial);
    assert_eq!(app.serial_display_text(), "DE*****01");
}

#[test]
fn serial_number_can_be_revealed_explicitly() {
    let mut app = GuiApp::demo();
    app.set_show_full_serial(true);

    assert_eq!(app.serial_display_text(), "DEMO-0001");
    assert_eq!(
        format_serial_for_display("SBSN1B50B00005", false),
        "SB**********05"
    );
}

#[test]
fn gui_app_prepares_native_screenshot_export_request() {
    let mut app = GuiApp::demo();
    let ctx = egui::Context::default();

    let path = app.request_screenshot_export(&ctx);

    assert_eq!(app.pending_screenshot_export_path(), Some(path.as_path()));
    assert!(path.starts_with(Path::new("docs").join("screenshots")));
    assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("png"));

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    assert!(file_name.starts_with("wattson-gui-"));

    let status = app.screenshot_export_status().unwrap_or_default();
    assert!(status.contains("等待") || status.contains("pending"));
}

#[test]
fn gui_app_saves_native_screenshot_png_and_reports_success() {
    let mut app = GuiApp::demo();
    let ctx = egui::Context::default();
    let path = temp_screenshot_path("gui-screenshot-success");

    app.begin_screenshot_export(path.clone());

    let mut raw_input = egui::RawInput::default();
    raw_input.events.push(egui::Event::Screenshot {
        viewport_id: egui::ViewportId::ROOT,
        user_data: egui::UserData::new(path.clone()),
        image: Arc::new(egui::ColorImage::filled(
            [4, 3],
            Color32::from_rgb(12, 34, 56),
        )),
    });

    <GuiApp as eframe::App>::raw_input_hook(&mut app, &ctx, &mut raw_input);

    let bytes = fs::read(&path).expect("native screenshot written");
    assert!(bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]));
    assert!(app.pending_screenshot_export_path().is_none());

    let status = app.screenshot_export_status().unwrap_or_default();
    assert!(status.contains("成功") || status.contains("saved"));

    let _ = fs::remove_file(&path);
    if let Some(parent) = path.parent() {
        let _ = fs::remove_dir(parent);
    }
}

#[test]
fn gui_app_reports_native_screenshot_export_failures() {
    let mut app = GuiApp::demo();
    let ctx = egui::Context::default();
    let blocker =
        std::env::temp_dir().join(format!("wattson-screenshot-blocker-{}", std::process::id()));
    let _ = fs::remove_file(&blocker);
    fs::write(&blocker, b"block export dir").expect("create blocker file");
    let path = blocker.join("native-export.png");

    app.begin_screenshot_export(path.clone());

    let mut raw_input = egui::RawInput::default();
    raw_input.events.push(egui::Event::Screenshot {
        viewport_id: egui::ViewportId::ROOT,
        user_data: egui::UserData::new(path.clone()),
        image: Arc::new(egui::ColorImage::filled([2, 2], Color32::WHITE)),
    });

    <GuiApp as eframe::App>::raw_input_hook(&mut app, &ctx, &mut raw_input);

    assert!(!path.exists());
    assert!(app.pending_screenshot_export_path().is_none());

    let status = app.screenshot_export_status().unwrap_or_default();
    assert!(status.contains("失败") || status.contains("failed"));

    let _ = fs::remove_file(blocker);
}
