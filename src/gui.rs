use std::time::{Duration, Instant};

use chrono::{Duration as ChronoDuration, Utc};
use eframe::egui::{self, Color32, RichText};
use eframe::{App, Frame, NativeOptions, Renderer};
use egui_plot::{Legend, Line, Plot, PlotPoints};

use crate::config::Config;
use crate::gui_settings::{ChartScaleMode, GuiSettings, ThemePreference};
use crate::history::History;
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

enum SourceMode {
    Demo { step: u64 },
    Live { handle: Option<PsuHandle> },
}

/// The desktop app / 桌面应用状态
pub struct GuiApp {
    config: Config,
    settings: GuiSettings,
    runtime: RuntimeState,
    history: History,
    source: SourceMode,
    started_at: Instant,
    last_sample_at: Instant,
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
            config,
            settings,
            started_at: Instant::now(),
            last_sample_at: Instant::now() - Duration::from_millis(1_000),
            persist_state,
        };

        app.set_poll_interval_ms(app.settings.poll_interval_ms);
        if demo {
            app.seed_demo_history(180);
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

    pub fn visible_series_count(&self) -> usize {
        usize::from(self.settings.show_ac_input) + usize::from(self.settings.show_dc_output)
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

    fn apply_theme(&self, ctx: &egui::Context) {
        ctx.set_theme(match self.settings.theme {
            ThemePreference::System => egui::ThemePreference::System,
            ThemePreference::Light => egui::ThemePreference::Light,
            ThemePreference::Dark => egui::ThemePreference::Dark,
        });
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

        let serial = if latest.snapshot.device.serial.is_empty() {
            "N/A"
        } else {
            latest.snapshot.device.serial.as_str()
        };

        egui::Panel::top("top_bar").show_inside(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("Wattson");
                ui.separator();
                ui.label(RichText::new(device_name).strong());
                ui.label(format!("S/N 序列号: {serial}"));
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

        let session_stats = self.runtime.stats();
        let total_wh = self.history.total_wh + self.runtime.session_wh();
        let total_duration_s = self.history.total_duration_s + self.runtime.session_duration_s();
        let total_kwh = total_wh / 1000.0;
        let total_cost = total_kwh * self.config.cost.price_per_kwh;
        let total_avg_w = if total_duration_s > 0.0 {
            total_wh / (total_duration_s / 3600.0)
        } else {
            latest.snapshot.power.ac_input_w
        };

        egui::Panel::left("summary_panel")
            .min_size(290.0)
            .show_inside(ui, |ui| {
                ui.heading("Telemetry 遥测");
                ui.separator();
                ui.label(format!(
                    "AC Input 输入: {:.1} W",
                    latest.snapshot.power.ac_input_w
                ));
                ui.label(format!(
                    "DC Output 输出: {:.1} W",
                    latest.snapshot.power.dc_output_est_w
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
                ui.heading("Energy 能耗");
                ui.label(format!("All-time 总电量: {:.4} kWh", total_kwh));
                ui.label(format!(
                    "All-time 总费用: {:.4} {}",
                    total_cost, self.config.cost.currency
                ));
                ui.label(format!(
                    "Session Avg 本次平均: {:.1} W",
                    session_stats.average_ac_input_w
                ));
                ui.label(format!("All-time Avg 总平均: {:.1} W", total_avg_w));
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
                    "Data age 数据龄期: {:.1}s",
                    latest.snapshot.meta.data_age_s
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
        self.ingest_sample(false);
        ctx.request_repaint_after(Duration::from_millis(self.settings.ui_refresh_ms));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut Frame) {
        self.render_top_bar(ui);
        self.render_summary_panel(ui);
        self.render_controls_panel(ui);
        self.render_chart(ui);
        self.render_status_bar(ui);
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
