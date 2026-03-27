use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{Duration as ChronoDuration, Utc};
use eframe::egui::{self, Color32, RichText};
use eframe::{App, Frame, NativeOptions, Renderer};
use egui_plot::{Legend, Line, Plot, PlotPoints};

use crate::config::Config;
use crate::gui_settings::{ChartScaleMode, GuiSettings, ThemePreference};
use crate::history::History;
use crate::protocol::FanMode;
use crate::runtime::{DemoGenerator, RuntimeState};
use crate::serial::PsuHandle;

/// Launch the native GUI / 启动原生 GUI
pub fn run(config: Config, handle: Option<PsuHandle>, demo: bool) -> Result<(), String> {
    let settings = GuiSettings::load();
    let window_size = [settings.window_width as f32, settings.window_height as f32];

    let native_options = NativeOptions {
        renderer: Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(window_size)
            .with_min_inner_size([1100.0, 720.0])
            .with_title("Wattson"),
        ..Default::default()
    };

    eframe::run_native(
        "Wattson",
        native_options,
        Box::new(move |cc| {
            configure_fonts(&cc.egui_ctx);
            Ok(Box::new(GuiApp::new(
                config.clone(),
                settings.clone(),
                handle,
                demo,
            )))
        }),
    )
    .map_err(|error| error.to_string())
}

/// Install a CJK-capable system font for bilingual labels / 安装支持中日韩字符的系统字体
pub fn configure_fonts(ctx: &egui::Context) {
    let Some(font_bytes) = load_localized_font_bytes() else {
        return;
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "system-cjk".to_string(),
        egui::FontData::from_owned(font_bytes).into(),
    );

    if let Some(proportional) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        proportional.insert(0, "system-cjk".to_string());
    }

    if let Some(monospace) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        monospace.push("system-cjk".to_string());
    }

    ctx.set_fonts(fonts);
}

fn load_localized_font_bytes() -> Option<Vec<u8>> {
    const CANDIDATES: &[&str] = &[
        "C:\\Windows\\Fonts\\simhei.ttf",
        "C:\\Windows\\Fonts\\simsunb.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.otf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
    ];

    CANDIDATES.iter().find_map(|path| std::fs::read(path).ok())
}

/// Format a serial number for display / 格式化序列号显示
pub fn format_serial_for_display(serial: &str, show_full: bool) -> String {
    if serial.is_empty() {
        return "N/A".to_string();
    }

    if show_full {
        return serial.to_string();
    }

    let char_count = serial.chars().count();
    if char_count <= 4 {
        return "*".repeat(char_count);
    }

    let prefix: String = serial.chars().take(2).collect();
    let suffix: String = serial
        .chars()
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    format!("{prefix}{}{suffix}", "*".repeat(char_count - 4))
}

enum SourceMode {
    Demo { step: u64 },
    Live { handle: Option<PsuHandle> },
}

/// Summary metrics shown in the GUI / GUI 摘要指标
#[derive(Debug, Clone)]
pub struct GuiSummaryMetrics {
    pub current_ac_input_w: f64,
    pub current_ac_smooth_w: f64,
    pub current_dc_output_w: f64,
    pub session_kwh: f64,
    pub session_cost: f64,
    pub session_duration_s: f64,
    pub session_avg_ac_input_w: f64,
    pub session_avg_dc_output_w: f64,
    pub all_time_kwh: f64,
    pub all_time_cost: f64,
    pub all_time_avg_ac_input_w: f64,
    pub currency: String,
}

/// GUI refresh performance stats / GUI 刷新性能指标
#[derive(Debug, Clone, Copy)]
pub struct GuiPerformanceStats {
    pub target_fps: f32,
    pub actual_fps: f32,
    pub frame_time_ms: f32,
}

/// The desktop app / 桌面应用状态
pub struct GuiApp {
    settings: GuiSettings,
    runtime: RuntimeState,
    history: History,
    source: SourceMode,
    selected_fan_mode: FanMode,
    manual_fan_pwm: u8,
    custom_curve_points: [(u8, u8); 3],
    last_fan_command_status: Option<String>,
    pending_screenshot_export: Option<PathBuf>,
    last_screenshot_export_status: Option<String>,
    started_at: Instant,
    last_sample_at: Instant,
    last_frame_at: Instant,
    frame_time_ema_s: f64,
    persist_state: bool,
}

impl GuiApp {
    pub fn demo() -> Self {
        Self::new_internal(Config::default(), GuiSettings::default(), None, true, false)
    }

    pub fn demo_with_theme(theme: ThemePreference) -> Self {
        let settings = GuiSettings {
            theme,
            ..GuiSettings::default()
        };
        Self::new_internal(Config::default(), settings, None, true, false)
    }

    pub fn new(
        config: Config,
        settings: GuiSettings,
        handle: Option<PsuHandle>,
        demo: bool,
    ) -> Self {
        Self::new_internal(config, settings, handle, demo, true)
    }

    fn new_internal(
        config: Config,
        settings: GuiSettings,
        handle: Option<PsuHandle>,
        demo: bool,
        persist_state: bool,
    ) -> Self {
        let mut app = Self {
            runtime: RuntimeState::new(config.cost.price_per_kwh, config.cost.currency.clone()),
            history: if demo {
                History::default()
            } else {
                History::load()
            },
            source: if demo {
                SourceMode::Demo { step: 0 }
            } else {
                SourceMode::Live { handle }
            },
            selected_fan_mode: FanMode::Auto,
            manual_fan_pwm: 50,
            custom_curve_points: [(40, 20), (60, 30), (80, 70)],
            last_fan_command_status: None,
            pending_screenshot_export: None,
            last_screenshot_export_status: None,
            settings,
            started_at: Instant::now(),
            last_sample_at: Instant::now() - Duration::from_millis(1_000),
            last_frame_at: Instant::now(),
            frame_time_ema_s: 0.0,
            persist_state,
        };

        app.set_poll_interval_ms(app.settings.poll_interval_ms);
        if demo {
            let chart_window_samples = (app.settings.chart_window_s.saturating_mul(1000)
                / app.settings.poll_interval_ms)
                + 1;
            app.seed_demo_history(chart_window_samples.max(180));
        } else {
            app.ingest_sample(true);
        }
        app
    }

    pub fn settings(&self) -> &GuiSettings {
        &self.settings
    }

    pub fn set_theme(&mut self, theme: ThemePreference) {
        self.settings.theme = theme;
    }

    pub fn set_series_visibility(&mut self, show_ac_input: bool, show_dc_output: bool) {
        self.settings.show_ac_input = show_ac_input;
        self.settings.show_dc_output = show_dc_output;
    }

    pub fn set_show_full_serial(&mut self, show_full_serial: bool) {
        self.settings.show_full_serial = show_full_serial;
    }

    pub fn serial_display_text(&self) -> String {
        self.runtime
            .latest()
            .map(|sample| {
                format_serial_for_display(
                    &sample.snapshot.device.serial,
                    self.settings.show_full_serial,
                )
            })
            .unwrap_or_else(|| "N/A".to_string())
    }

    pub fn pending_screenshot_export_path(&self) -> Option<&Path> {
        self.pending_screenshot_export.as_deref()
    }

    pub fn screenshot_export_status(&self) -> Option<&str> {
        self.last_screenshot_export_status.as_deref()
    }

    pub fn begin_screenshot_export(&mut self, path: PathBuf) {
        self.pending_screenshot_export = Some(path.clone());
        self.last_screenshot_export_status = Some(format!(
            "Screenshot export pending (截图导出等待中): {}",
            path.display()
        ));
    }

    pub fn request_screenshot_export(&mut self, ctx: &egui::Context) -> PathBuf {
        let path = default_screenshot_export_path();
        self.begin_screenshot_export(path.clone());
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
            path.clone(),
        )));
        path
    }

    pub fn set_ui_refresh_ms(&mut self, ui_refresh_ms: u64) {
        self.settings.ui_refresh_ms = ui_refresh_ms.clamp(16, 1_000);
    }

    pub fn set_poll_interval_ms(&mut self, poll_interval_ms: u64) {
        let clamped = poll_interval_ms.clamp(200, 5_000);
        self.settings.poll_interval_ms = clamped;

        if let SourceMode::Live {
            handle: Some(handle),
        } = &self.source
        {
            handle.set_poll_ms(clamped);
        }
    }

    pub fn sync_window_size(&mut self, width: f32, height: f32) {
        self.settings.window_width = width.round().max(1.0) as u32;
        self.settings.window_height = height.round().max(1.0) as u32;
    }

    pub fn visible_series_count(&self) -> usize {
        usize::from(self.settings.show_ac_input) + usize::from(self.settings.show_dc_output)
    }

    pub fn summary_metrics(&self) -> Option<GuiSummaryMetrics> {
        let latest = self.runtime.latest()?;
        let session = self.runtime.stats();
        let all_time = self.runtime.all_time_stats(&self.history);

        Some(GuiSummaryMetrics {
            current_ac_input_w: latest.snapshot.power.ac_input_w,
            current_ac_smooth_w: latest.snapshot.power.ac_input_avg_w,
            current_dc_output_w: latest.snapshot.power.dc_output_est_w,
            session_kwh: session.total_kwh,
            session_cost: session.total_cost,
            session_duration_s: session.duration_s,
            session_avg_ac_input_w: session.average_ac_input_w,
            session_avg_dc_output_w: session.average_dc_output_w,
            all_time_kwh: all_time.total_kwh,
            all_time_cost: all_time.total_cost,
            all_time_avg_ac_input_w: all_time.average_ac_input_w,
            currency: all_time.currency,
        })
    }

    pub fn performance_stats(&self) -> GuiPerformanceStats {
        let target_fps = 1000.0 / self.settings.ui_refresh_ms as f32;
        if !self.persist_state {
            return GuiPerformanceStats {
                target_fps,
                actual_fps: target_fps,
                frame_time_ms: self.settings.ui_refresh_ms as f32,
            };
        }

        let frame_time_ms = (self.frame_time_ema_s * 1000.0) as f32;
        let actual_fps = if self.frame_time_ema_s > 0.0 {
            (1.0 / self.frame_time_ema_s) as f32
        } else {
            0.0
        };

        GuiPerformanceStats {
            target_fps,
            actual_fps,
            frame_time_ms,
        }
    }

    fn seed_demo_history(&mut self, sample_count: u64) {
        let SourceMode::Demo { step } = &mut self.source else {
            return;
        };

        let now = Utc::now();
        let poll_ms = self.settings.poll_interval_ms as i64;

        for index in 0..sample_count {
            let age_ms = (sample_count - 1 - index) as i64 * poll_ms;
            let timestamp = now - ChronoDuration::milliseconds(age_ms);
            self.runtime
                .push_snapshot(timestamp, DemoGenerator::sample_at(index));
        }

        *step = sample_count;
        self.last_sample_at = Instant::now();
    }

    fn ingest_sample(&mut self, force: bool) {
        if !force
            && self.last_sample_at.elapsed() < Duration::from_millis(self.settings.poll_interval_ms)
        {
            return;
        }

        let snapshot = match &mut self.source {
            SourceMode::Demo { step } => {
                let snapshot = DemoGenerator::sample_at(*step);
                *step += 1;
                snapshot
            }
            SourceMode::Live { handle } => handle
                .as_ref()
                .map(|live_handle| live_handle.latest())
                .unwrap_or_default(),
        };

        self.runtime.push_snapshot(Utc::now(), snapshot);
        self.last_sample_at = Instant::now();
    }

    fn update_frame_timing(&mut self) {
        if !self.persist_state {
            return;
        }

        let now = Instant::now();
        let elapsed_s = now.duration_since(self.last_frame_at).as_secs_f64();
        self.last_frame_at = now;

        if elapsed_s <= 0.0 {
            return;
        }

        self.frame_time_ema_s = if self.frame_time_ema_s == 0.0 {
            elapsed_s
        } else {
            self.frame_time_ema_s * 0.85 + elapsed_s * 0.15
        };
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        ctx.set_theme(match self.settings.theme {
            ThemePreference::System => egui::ThemePreference::System,
            ThemePreference::Light => egui::ThemePreference::Light,
            ThemePreference::Dark => egui::ThemePreference::Dark,
        });
    }

    fn fan_controls_enabled(&self) -> bool {
        matches!(self.source, SourceMode::Live { handle: Some(_) })
    }

    fn with_live_handle<R>(&self, f: impl FnOnce(&PsuHandle) -> R) -> Option<R> {
        match &self.source {
            SourceMode::Live {
                handle: Some(handle),
            } => Some(f(handle)),
            _ => None,
        }
    }

    fn apply_selected_fan_mode(&mut self) {
        let message =
            match self.with_live_handle(|handle| handle.set_fan_mode(self.selected_fan_mode)) {
                Some(Ok(())) => format!(
                    "Fan mode applied: {} (风扇模式已应用: {})",
                    self.selected_fan_mode,
                    self.selected_fan_mode.label()
                ),
                Some(Err(error)) => format!("Fan mode failed (风扇模式失败): {error}"),
                None => "Live device required for fan control (风扇控制需要实时设备)".to_string(),
            };
        self.last_fan_command_status = Some(message);
    }

    fn apply_manual_fan_pwm(&mut self) {
        let pwm = self.manual_fan_pwm;
        let message = match self.with_live_handle(|handle| handle.set_fan_pwm(pwm)) {
            Some(Ok(())) => format!("Fan PWM applied: {pwm}% (固定占空比已应用: {pwm}%)"),
            Some(Err(error)) => format!("Fan PWM failed (固定占空比失败): {error}"),
            None => "Live device required for fan control (风扇控制需要实时设备)".to_string(),
        };
        self.last_fan_command_status = Some(message);
    }

    fn apply_custom_curve(&mut self) {
        let curve = vec![
            (0, 0),
            self.custom_curve_points[0],
            self.custom_curve_points[1],
            self.custom_curve_points[2],
            (100, 100),
        ];

        let message = match self.with_live_handle(|handle| handle.set_fan_curve(curve.clone())) {
            Some(Ok(())) => format!(
                "Custom curve applied (自定义曲线已应用): {:?}",
                self.custom_curve_points
            ),
            Some(Err(error)) => format!("Custom curve failed (自定义曲线失败): {error}"),
            None => "Live device required for fan control (风扇控制需要实时设备)".to_string(),
        };
        self.last_fan_command_status = Some(message);
    }

    fn finish_screenshot_export(&mut self, user_data: &egui::UserData, image: &egui::ColorImage) {
        let path = screenshot_path_from_user_data(user_data)
            .or_else(|| self.pending_screenshot_export.clone());

        let Some(path) = path else {
            self.pending_screenshot_export = None;
            self.last_screenshot_export_status = Some(
                "Screenshot export failed (截图导出失败): missing export path / 缺少导出路径"
                    .to_string(),
            );
            return;
        };

        match save_color_image_png(&path, image) {
            Ok(()) => {
                self.pending_screenshot_export = None;
                self.last_screenshot_export_status = Some(format!(
                    "Screenshot exported successfully (截图导出成功): {}",
                    path.display()
                ));
            }
            Err(error) => {
                self.pending_screenshot_export = None;
                self.last_screenshot_export_status = Some(format!(
                    "Screenshot export failed (截图导出失败): {error}"
                ));
            }
        }
    }

    fn render_top_bar(&mut self, ui: &mut egui::Ui) {
        let Some(latest) = self.runtime.latest() else {
            return;
        };

        let device_name = if latest.snapshot.device.model.is_empty() {
            "Unknown PSU / 未知电源"
        } else {
            latest.snapshot.device.model.as_str()
        };

        let serial = self.serial_display_text();

        egui::Panel::top("top_bar").show_inside(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("Wattson");
                ui.separator();
                ui.label(RichText::new(device_name).strong());
                ui.label(format!("S/N 序列号: {serial}"));
                ui.checkbox(
                    &mut self.settings.show_full_serial,
                    "Show Full S/N 显示完整序列号",
                );
                ui.separator();
                ui.label(format!(
                    "Source 数据源: {}",
                    match self.source {
                        SourceMode::Demo { .. } => "Demo 演示",
                        SourceMode::Live { .. } => "Live 实时",
                    }
                ));
                ui.separator();

                egui::ComboBox::from_label("Theme 主题")
                    .selected_text(match self.settings.theme {
                        ThemePreference::System => "System 跟随系统",
                        ThemePreference::Light => "Light 浅色",
                        ThemePreference::Dark => "Dark 深色",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.settings.theme,
                            ThemePreference::System,
                            "System 跟随系统",
                        );
                        ui.selectable_value(
                            &mut self.settings.theme,
                            ThemePreference::Light,
                            "Light 浅色",
                        );
                        ui.selectable_value(
                            &mut self.settings.theme,
                            ThemePreference::Dark,
                            "Dark 深色",
                        );
                    });
            });
        });
    }

    fn render_summary_panel(&mut self, ui: &mut egui::Ui) {
        let Some(latest) = self.runtime.latest() else {
            return;
        };
        let Some(metrics) = self.summary_metrics() else {
            return;
        };
        let session_time = format_duration_hms(metrics.session_duration_s);

        egui::Panel::left("summary_panel")
            .min_size(290.0)
            .show_inside(ui, |ui| {
                ui.heading("Telemetry 遥测");
                ui.separator();
                ui.label(format!(
                    "AC Input 输入: {:.1} W",
                    metrics.current_ac_input_w
                ));
                ui.label(format!(
                    "AC Smooth 平滑输入: {:.1} W",
                    metrics.current_ac_smooth_w
                ));
                ui.label(format!(
                    "DC Output 输出: {:.1} W",
                    metrics.current_dc_output_w
                ));
                ui.label(format!(
                    "Efficiency 效率: {:.1} %",
                    latest.snapshot.power.efficiency_pct
                ));
                ui.label(format!(
                    "Main Temp 主温度: {:.1} C",
                    latest.snapshot.thermal.temp_main_c
                ));
                ui.label(format!("Fan 风扇: {} RPM", latest.snapshot.fan.rpm));
                ui.separator();
                ui.heading("Session 本次");
                ui.label(format!("Session 电量: {:.4} kWh", metrics.session_kwh));
                ui.label(format!(
                    "Session 费用: {:.4} {}",
                    metrics.session_cost, metrics.currency
                ));
                ui.label(format!("Session 时长: {session_time}"));
                ui.label(format!(
                    "Session Avg AC 本次输入平均: {:.1} W",
                    metrics.session_avg_ac_input_w
                ));
                ui.label(format!(
                    "Session Avg DC 本次输出平均: {:.1} W",
                    metrics.session_avg_dc_output_w
                ));
                ui.separator();
                ui.heading("All-time 总计");
                ui.label(format!("All-time 总电量: {:.4} kWh", metrics.all_time_kwh));
                ui.label(format!(
                    "All-time 总费用: {:.4} {}",
                    metrics.all_time_cost, metrics.currency
                ));
                ui.label(format!(
                    "All-time Avg 总平均输入: {:.1} W",
                    metrics.all_time_avg_ac_input_w
                ));
            });
    }

    fn render_controls_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("controls_panel")
            .min_size(300.0)
            .show_inside(ui, |ui| {
                ui.heading("Controls 控制");
                ui.separator();

                ui.label("Chart Window 图表窗口");
                ui.add(egui::Slider::new(&mut self.settings.chart_window_s, 30..=600).suffix(" s"));

                ui.label("UI Refresh 刷新帧率");
                let mut refresh_ms = self.settings.ui_refresh_ms;
                if ui
                    .add(egui::Slider::new(&mut refresh_ms, 16..=1_000).suffix(" ms"))
                    .changed()
                {
                    self.set_ui_refresh_ms(refresh_ms);
                }

                ui.label("Serial Poll 串口轮询");
                let mut poll_ms = self.settings.poll_interval_ms;
                if ui
                    .add(egui::Slider::new(&mut poll_ms, 200..=5_000).suffix(" ms"))
                    .changed()
                {
                    self.set_poll_interval_ms(poll_ms);
                }

                ui.separator();
                ui.label("Series 曲线");
                ui.checkbox(&mut self.settings.show_ac_input, "AC Input 输入");
                ui.checkbox(&mut self.settings.show_dc_output, "DC Output 输出");

                ui.separator();
                ui.label("Scale 缩放");
                ui.radio_value(
                    &mut self.settings.chart_scale,
                    ChartScaleMode::Auto,
                    "Auto 自动",
                );
                ui.radio_value(
                    &mut self.settings.chart_scale,
                    ChartScaleMode::ZeroBased,
                    "Zero Based 零基线",
                );

                ui.separator();
                ui.label(format!(
                    "Runtime 运行时: {:.0}s",
                    self.started_at.elapsed().as_secs_f64()
                ));
                ui.label(format!(
                    "Packets 数据包: {}",
                    self.runtime
                        .latest()
                        .map(|sample| sample.snapshot.meta.packet_count)
                        .unwrap_or(0)
                ));
                let perf = self.performance_stats();
                ui.separator();
                ui.label("Performance 性能");
                ui.label(format!("Target FPS 目标帧率: {:.1}", perf.target_fps));
                ui.label(format!("Actual FPS 实际帧率: {:.1}", perf.actual_fps));
                ui.label(format!("Frame Time 帧耗时: {:.1} ms", perf.frame_time_ms));

                ui.separator();
                ui.heading("Screenshot 截图导出");
                if ui.button("Export Screenshot 导出截图").clicked() {
                    let ctx = ui.ctx().clone();
                    self.request_screenshot_export(&ctx);
                }
                if let Some(status) = &self.last_screenshot_export_status {
                    ui.label(status);
                }

                ui.separator();
                ui.heading("Fan Control 风扇控制");

                if !self.fan_controls_enabled() {
                    ui.label("Demo / 离线模式下禁用写入控制");
                }

                ui.add_enabled_ui(self.fan_controls_enabled(), |ui| {
                    egui::ComboBox::from_label("Fan Mode 风扇模式")
                        .selected_text(self.selected_fan_mode.label())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.selected_fan_mode,
                                FanMode::Auto,
                                FanMode::Auto.label(),
                            );
                            ui.selectable_value(
                                &mut self.selected_fan_mode,
                                FanMode::Silent,
                                FanMode::Silent.label(),
                            );
                            ui.selectable_value(
                                &mut self.selected_fan_mode,
                                FanMode::Performance,
                                FanMode::Performance.label(),
                            );
                            ui.selectable_value(
                                &mut self.selected_fan_mode,
                                FanMode::Custom,
                                FanMode::Custom.label(),
                            );
                            ui.selectable_value(
                                &mut self.selected_fan_mode,
                                FanMode::Clean,
                                FanMode::Clean.label(),
                            );
                        });

                    if ui.button("Apply Mode 应用模式").clicked() {
                        self.apply_selected_fan_mode();
                    }

                    ui.separator();
                    ui.label("Manual PWM 固定占空比");
                    ui.add(egui::Slider::new(&mut self.manual_fan_pwm, 0..=100).suffix(" %"));
                    if ui.button("Apply PWM 应用占空比").clicked() {
                        self.apply_manual_fan_pwm();
                    }

                    ui.separator();
                    ui.label("Custom Curve 自定义曲线");

                    let mut p1 = self.custom_curve_points[0];
                    ui.horizontal(|ui| {
                        ui.label("P1 点1");
                        ui.add(egui::Slider::new(&mut p1.0, 5..=60).suffix(" C"));
                        ui.add(egui::Slider::new(&mut p1.1, 0..=100).suffix(" %"));
                    });
                    self.custom_curve_points[0] = p1;

                    let mut p2 = self.custom_curve_points[1];
                    ui.horizontal(|ui| {
                        ui.label("P2 点2");
                        let min = (p1.0.saturating_add(5)).min(85);
                        let max = 85u8.max(min);
                        ui.add(egui::Slider::new(&mut p2.0, min..=max).suffix(" C"));
                        ui.add(egui::Slider::new(&mut p2.1, 0..=100).suffix(" %"));
                    });
                    self.custom_curve_points[1] = p2;

                    let mut p3 = self.custom_curve_points[2];
                    ui.horizontal(|ui| {
                        ui.label("P3 点3");
                        let min = (p2.0.saturating_add(5)).min(95);
                        let max = 95u8.max(min);
                        ui.add(egui::Slider::new(&mut p3.0, min..=max).suffix(" C"));
                        ui.add(egui::Slider::new(&mut p3.1, 0..=100).suffix(" %"));
                    });
                    self.custom_curve_points[2] = p3;

                    if ui.button("Apply Curve 应用曲线").clicked() {
                        self.apply_custom_curve();
                    }
                });

                if let Some(status) = &self.last_fan_command_status {
                    ui.separator();
                    ui.label(status);
                }
            });
    }

    fn render_chart(&mut self, ui: &mut egui::Ui) {
        let samples = self
            .runtime
            .samples_in_window(self.settings.chart_window_s as i64);
        let Some(latest) = samples.last() else {
            return;
        };
        let latest_timestamp = latest.timestamp;

        let ac_points: Vec<[f64; 2]> = samples
            .iter()
            .map(|sample| {
                [
                    (sample.timestamp - latest_timestamp).num_milliseconds() as f64 / 1000.0,
                    sample.snapshot.power.ac_input_w,
                ]
            })
            .collect();
        let dc_points: Vec<[f64; 2]> = samples
            .iter()
            .map(|sample| {
                [
                    (sample.timestamp - latest_timestamp).num_milliseconds() as f64 / 1000.0,
                    sample.snapshot.power.dc_output_est_w,
                ]
            })
            .collect();

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.heading("Native GPU Dashboard 原生 GPU 仪表盘");
            ui.label("Rendered by egui + wgpu / 由 egui + wgpu 渲染");
            ui.separator();

            let mut plot = Plot::new("power_plot")
                .legend(Legend::default())
                .height(ui.available_height() - 80.0)
                .include_x(-(self.settings.chart_window_s as f64))
                .include_x(0.0);

            if self.settings.chart_scale == ChartScaleMode::ZeroBased {
                plot = plot.include_y(0.0);
            }

            plot.show(ui, |plot_ui| {
                if self.settings.show_ac_input {
                    plot_ui.line(
                        Line::new("AC Input 输入", PlotPoints::from(ac_points.clone()))
                            .color(Color32::from_rgb(220, 64, 64)),
                    );
                }
                if self.settings.show_dc_output {
                    plot_ui.line(
                        Line::new("DC Output 输出", PlotPoints::from(dc_points.clone()))
                            .color(Color32::from_rgb(72, 160, 255)),
                    );
                }
            });
        });
    }

    fn render_status_bar(&mut self, ui: &mut egui::Ui) {
        let Some(latest) = self.runtime.latest() else {
            return;
        };
        let perf = self.performance_stats();

        let connected_text = if latest.snapshot.meta.connected {
            RichText::new("CONNECTED 已连接").color(Color32::LIGHT_GREEN)
        } else {
            RichText::new("DISCONNECTED 未连接").color(Color32::LIGHT_RED)
        };

        egui::Panel::bottom("status_bar").show_inside(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(connected_text.clone());
                ui.separator();
                ui.label(format!(
                    "Poll 轮询: {} ms | Refresh 刷新: {} ms",
                    self.settings.poll_interval_ms, self.settings.ui_refresh_ms
                ));
                ui.separator();
                ui.label(format!(
                    "FPS 帧率: {:.1}/{:.1}",
                    perf.actual_fps, perf.target_fps
                ));
                ui.separator();
                ui.label(format!(
                    "Data age 数据龄期: {}",
                    format_data_age(latest.snapshot.meta.data_age_s)
                ));
                ui.separator();
                ui.label(format!("Errors 错误: {}", latest.snapshot.meta.error_count));
            });
        });
    }
}

impl App for GuiApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        self.apply_theme(ctx);
        let viewport_size = ctx.content_rect().size();
        self.sync_window_size(viewport_size.x, viewport_size.y);
        self.ingest_sample(false);
        self.update_frame_timing();
        ctx.request_repaint_after(Duration::from_millis(self.settings.ui_refresh_ms));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut Frame) {
        self.render_top_bar(ui);
        self.render_summary_panel(ui);
        self.render_controls_panel(ui);
        self.render_chart(ui);
        self.render_status_bar(ui);
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        let screenshot_events: Vec<(egui::UserData, std::sync::Arc<egui::ColorImage>)> =
            raw_input
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::Screenshot {
                        user_data, image, ..
                    } => Some((user_data.clone(), image.clone())),
                    _ => None,
                })
                .collect();

        raw_input
            .events
            .retain(|event| !matches!(event, egui::Event::Screenshot { .. }));

        for (user_data, image) in screenshot_events {
            self.finish_screenshot_export(&user_data, &image);
        }
    }
}

impl Drop for GuiApp {
    fn drop(&mut self) {
        if !self.persist_state {
            return;
        }

        let _ = self.settings.save();

        if matches!(self.source, SourceMode::Live { .. }) {
            self.history
                .finish_session(self.runtime.session_wh(), self.runtime.session_duration_s());
            let _ = self.history.save();
        }

        if let SourceMode::Live { handle } = &mut self.source {
            if let Some(handle) = handle.take() {
                handle.stop();
            }
        }
    }
}

fn format_duration_hms(duration_s: f64) -> String {
    let duration_s = duration_s.max(0.0);
    let hours = (duration_s / 3600.0) as u64;
    let minutes = ((duration_s % 3600.0) / 60.0) as u64;
    let seconds = (duration_s % 60.0) as u64;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn format_data_age(data_age_s: f64) -> String {
    if !data_age_s.is_finite() {
        return "N/A 未知".to_string();
    }

    format!("{data_age_s:.1}s")
}

fn default_screenshot_export_path() -> PathBuf {
    let now = Utc::now();
    let file_name = format!(
        "wattson-gui-{}-{:03}.png",
        now.format("%Y%m%d-%H%M%S"),
        now.timestamp_subsec_millis()
    );
    PathBuf::from("docs").join("screenshots").join(file_name)
}

fn screenshot_path_from_user_data(user_data: &egui::UserData) -> Option<PathBuf> {
    user_data
        .data
        .as_ref()
        .and_then(|data| data.downcast_ref::<PathBuf>())
        .cloned()
}

fn save_color_image_png(path: &Path, image: &egui::ColorImage) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create dir {}: {error}", parent.display()))?;
    }

    let rgba: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|pixel| pixel.to_srgba_unmultiplied())
        .collect();

    let buffer = image::RgbaImage::from_raw(image.size[0] as u32, image.size[1] as u32, rgba)
        .ok_or_else(|| {
            format!(
                "Failed to build RGBA buffer for screenshot (截图缓冲区构建失败): {}x{}",
                image.size[0], image.size[1]
            )
        })?;

    buffer
        .save_with_format(path, image::ImageFormat::Png)
        .map_err(|error| format!("Failed to save {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_data_age_hides_non_finite_values() {
        assert_eq!(format_data_age(f64::INFINITY), "N/A 未知");
        assert_eq!(format_data_age(f64::NAN), "N/A 未知");
    }

    #[test]
    fn default_screenshot_export_path_targets_docs_screenshots() {
        let path = default_screenshot_export_path();

        assert!(path.starts_with(Path::new("docs").join("screenshots")));
        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("png"));
    }
}
