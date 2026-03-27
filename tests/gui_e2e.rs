use std::fs;
use std::path::PathBuf;

use egui_kittest::{Harness, SnapshotOptions};
use wattson::gui::{configure_fonts, GuiApp};
use wattson::gui_settings::ThemePreference;

fn snapshot_dir() -> PathBuf {
    PathBuf::from("tests").join("snapshots").join("wattson")
}

fn screenshot_dir() -> PathBuf {
    PathBuf::from("docs").join("screenshots")
}

// Pixel snapshots are canonically generated on Windows.
// 像素级快照以 Windows 为基准，其他平台只保留编译覆盖，避免跨平台渲染差异导致误报。
#[test]
#[cfg_attr(
    not(target_os = "windows"),
    ignore = "pixel snapshots are canonicalized on Windows only"
)]
fn gui_demo_snapshots_cover_dark_and_light_modes() {
    let _ = fs::create_dir_all(snapshot_dir());
    let _ = fs::create_dir_all(screenshot_dir());

    for (name, theme) in [
        ("native-gui-dark", ThemePreference::Dark),
        ("native-gui-light", ThemePreference::Light),
    ] {
        let mut harness = Harness::builder()
            .with_size([1440.0, 900.0])
            .wgpu()
            .build_eframe(|cc| {
                configure_fonts(&cc.egui_ctx);
                GuiApp::demo_with_theme(theme)
            });

        harness
            .try_snapshot_options(
                name,
                &SnapshotOptions::default().output_path(snapshot_dir()),
            )
            .expect("snapshot matches");

        let image = harness.render().expect("rendered image");
        let screenshot_path = screenshot_dir().join(format!("{name}.png"));
        image.save(&screenshot_path).expect("saved screenshot");

        let metadata = fs::metadata(&screenshot_path).expect("screenshot metadata");
        assert!(metadata.len() > 0);
    }
}
