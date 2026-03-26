use wattson::gui::GuiApp;
use wattson::gui_settings::ThemePreference;

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
