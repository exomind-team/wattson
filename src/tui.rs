use std::collections::VecDeque;
use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::prelude::*;
use ratatui::widgets::{
    Axis, Block, Borders, Cell, Chart, Dataset, GraphType, Paragraph, Row, Table,
};

use crate::config::Config;
use crate::data::PsuSnapshot;
use crate::serial::PsuHandle;

/// Maximum number of samples in chart history
const CHART_HISTORY_LEN: usize = 120;

/// Cost accumulator for the TUI session
struct CostState {
    total_wh: f64,
    last_sample: Instant,
    start_time: Instant,
    price_per_kwh: f64,
    currency: String,
}

/// Time-series data for charts
struct ChartHistory {
    ac_power: VecDeque<f64>,
    dc_power: VecDeque<f64>,
    sample_count: u64,
}

impl ChartHistory {
    fn new() -> Self {
        Self {
            ac_power: VecDeque::with_capacity(CHART_HISTORY_LEN + 1),
            dc_power: VecDeque::with_capacity(CHART_HISTORY_LEN + 1),
            sample_count: 0,
        }
    }

    fn push(&mut self, ac: f64, dc: f64) {
        self.ac_power.push_back(ac);
        self.dc_power.push_back(dc);
        if self.ac_power.len() > CHART_HISTORY_LEN {
            self.ac_power.pop_front();
        }
        if self.dc_power.len() > CHART_HISTORY_LEN {
            self.dc_power.pop_front();
        }
        self.sample_count += 1;
    }

    fn ac_data_points(&self) -> Vec<(f64, f64)> {
        self.ac_power
            .iter()
            .enumerate()
            .map(|(i, &v)| (i as f64, v))
            .collect()
    }

    fn dc_data_points(&self) -> Vec<(f64, f64)> {
        self.dc_power
            .iter()
            .enumerate()
            .map(|(i, &v)| (i as f64, v))
            .collect()
    }

    fn max_power(&self) -> f64 {
        let ac_max = self.ac_power.iter().cloned().fold(0.0_f64, f64::max);
        let dc_max = self.dc_power.iter().cloned().fold(0.0_f64, f64::max);
        ac_max.max(dc_max).max(50.0) // minimum 50W scale
    }

    fn min_power(&self) -> f64 {
        let ac_min = self
            .ac_power
            .iter()
            .cloned()
            .filter(|&v| v > 0.0)
            .fold(f64::INFINITY, f64::min);
        let dc_min = self
            .dc_power
            .iter()
            .cloned()
            .filter(|&v| v > 0.0)
            .fold(f64::INFINITY, f64::min);
        let min = ac_min.min(dc_min);
        if min.is_infinite() {
            0.0
        } else {
            min
        }
    }
}

/// Chart Y-axis mode
#[derive(Clone, Copy, PartialEq)]
enum ChartScale {
    /// Y starts from 0W (full range, honest)
    Zero,
    /// Y auto-zooms to data range (shows detail)
    Auto,
}

/// Run the TUI dashboard
pub fn run(handle: &PsuHandle, config: &Config, refresh_ms: u64) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut cost = CostState {
        total_wh: 0.0,
        last_sample: Instant::now(),
        start_time: Instant::now(),
        price_per_kwh: config.cost.price_per_kwh,
        currency: config.cost.currency.clone(),
    };

    let mut history = ChartHistory::new();
    let mut chart_scale = ChartScale::Auto;
    let tick_rate = Duration::from_millis(refresh_ms);
    let mut last_tick = Instant::now();

    loop {
        let snap = handle.latest();

        // Accumulate energy: power(W) * time(h) = Wh
        let elapsed_h = cost.last_sample.elapsed().as_secs_f64() / 3600.0;
        cost.total_wh += snap.power.ac_input_w * elapsed_h;
        cost.last_sample = Instant::now();

        // Push to chart history
        history.push(snap.power.ac_input_w, snap.power.dc_output_est_w);

        let total_kwh = cost.total_wh / 1000.0;
        let total_cost = total_kwh * cost.price_per_kwh;
        let duration_s = cost.start_time.elapsed().as_secs_f64();
        let currency = cost.currency.clone();
        let price = cost.price_per_kwh;

        terminal.draw(|f| {
            render_ui(
                f,
                &snap,
                total_kwh,
                total_cost,
                &currency,
                price,
                duration_s,
                &history,
                snap.power.ac_input_avg_w,
                chart_scale,
            );
        })?;

        // Handle input — drain all pending events to avoid mouse scroll triggering rapid redraws
        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        let mut should_quit = false;
        if event::poll(timeout)? {
            loop {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('q') => should_quit = true,
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            should_quit = true
                        }
                        KeyCode::Char('z') => {
                            chart_scale = match chart_scale {
                                ChartScale::Zero => ChartScale::Auto,
                                ChartScale::Auto => ChartScale::Zero,
                            };
                        }
                        KeyCode::Char('+') | KeyCode::Char('=') => {
                            let ms = handle.poll_ms();
                            if ms > 100 {
                                handle.set_poll_ms(ms - 100);
                            }
                        }
                        KeyCode::Char('-') => {
                            let ms = handle.poll_ms();
                            handle.set_poll_ms(ms + 100);
                        }
                        _ => {}
                    }
                }
                // Drain remaining events without blocking
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        if should_quit {
            break;
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn render_ui(
    f: &mut Frame,
    snap: &PsuSnapshot,
    total_kwh: f64,
    total_cost: f64,
    currency: &str,
    price: f64,
    duration_s: f64,
    history: &ChartHistory,
    ac_avg_w: f64,
    chart_scale: ChartScale,
) {
    let area = f.area();

    // Main layout: device info | middle panels | chart | bottom panels | status bar
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // device info
            Constraint::Length(9), // middle section (power + DC)
            Constraint::Min(8),    // chart
            Constraint::Length(9), // bottom section (thermal + cost)
            Constraint::Length(1), // status bar
        ])
        .split(area);

    // Device info panel
    let connected_str = if snap.meta.connected {
        "CONNECTED"
    } else {
        "DISCONNECTED"
    };
    let device_text = format!(
        " {} | S/N: {} | Status: {}",
        if snap.device.model.is_empty() {
            "Unknown PSU"
        } else {
            &snap.device.model
        },
        if snap.device.serial.is_empty() {
            "N/A"
        } else {
            &snap.device.serial
        },
        connected_str,
    );
    let device_style = if snap.meta.connected {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::Red)
    };
    let device_block = Paragraph::new(device_text)
        .style(device_style)
        .block(Block::default().borders(Borders::ALL).title(" Device "));
    f.render_widget(device_block, main_chunks[0]);

    // Middle section: power (left) + DC rails (right)
    let middle_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_chunks[1]);

    render_power_panel(f, snap, middle_chunks[0]);
    render_dc_panel(f, snap, middle_chunks[1]);

    // Chart section
    render_power_chart(f, history, chart_scale, main_chunks[2]);

    // Bottom section: thermal+fan (left) + cost (right)
    let bottom_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_chunks[3]);

    render_thermal_panel(f, snap, bottom_chunks[0]);
    render_cost_panel(
        f,
        total_kwh,
        total_cost,
        currency,
        price,
        duration_s,
        ac_avg_w,
        bottom_chunks[1],
    );

    // Status bar — show hotkeys and poll speed
    let status_text = format!(
        " q:quit  z:scale  +/-:speed({}ms)  Age:{:.0}s",
        snap.meta.poll_ms, snap.meta.data_age_s,
    );
    let status = Paragraph::new(status_text).style(Style::default().fg(Color::DarkGray));
    f.render_widget(status, main_chunks[4]);
}

fn render_power_panel(f: &mut Frame, snap: &PsuSnapshot, area: Rect) {
    let ac_color = if snap.power.ac_input_w > 500.0 {
        Color::Red
    } else if snap.power.ac_input_w > 200.0 {
        Color::Yellow
    } else {
        Color::Green
    };

    let lines = vec![
        Line::from(vec![
            Span::raw("  AC Input:  "),
            Span::styled(
                format!("{:>7.1} W", snap.power.ac_input_w),
                Style::default().fg(ac_color).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(format!("  AC Avg:    {:>7.1} W", snap.power.ac_input_avg_w)),
        Line::from(format!(
            "  DC Output: {:>7.1} W",
            snap.power.dc_output_est_w
        )),
        Line::from(format!("  Efficiency:{:>6.1} %", snap.power.efficiency_pct)),
        Line::from(format!(
            "  AC: {:>5.1}V {:>4.1}Hz",
            snap.ac.voltage_v, snap.ac.frequency_hz
        )),
    ];

    let block =
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Power "));
    f.render_widget(block, area);
}

fn render_dc_panel(f: &mut Frame, snap: &PsuSnapshot, area: Rect) {
    let rail_color = |actual: f64, nominal: f64| -> Color {
        let deviation = ((actual - nominal) / nominal).abs();
        if deviation > 0.05 {
            Color::Red
        } else if deviation > 0.03 {
            Color::Yellow
        } else {
            Color::Green
        }
    };

    let rows = vec![
        Row::new(vec![
            Cell::from("+12V"),
            Cell::from(format!("{:.3} V", snap.dc.volt_12v))
                .style(Style::default().fg(rail_color(snap.dc.volt_12v, 12.0))),
            Cell::from(format!("{:.2} A", snap.dc.curr_12v_a)),
            Cell::from(format!("{:.1} W", snap.dc.power_12v_w)),
        ]),
        Row::new(vec![
            Cell::from("+5V"),
            Cell::from(format!("{:.3} V", snap.dc.volt_5v))
                .style(Style::default().fg(rail_color(snap.dc.volt_5v, 5.0))),
            Cell::from(format!("{:.2} A", snap.dc.curr_5v_a)),
            Cell::from(format!("{:.1} W", snap.dc.power_5v_w)),
        ]),
        Row::new(vec![
            Cell::from("+3.3V"),
            Cell::from(format!("{:.3} V", snap.dc.volt_3v3))
                .style(Style::default().fg(rail_color(snap.dc.volt_3v3, 3.3))),
            Cell::from(format!("{:.2} A", snap.dc.curr_3v3_a)),
            Cell::from(format!("{:.1} W", snap.dc.power_3v3_w)),
        ]),
        Row::new(vec![
            Cell::from("+5VSB"),
            Cell::from(format!("{:.3} V", snap.dc.volt_5vsb))
                .style(Style::default().fg(rail_color(snap.dc.volt_5vsb, 5.0))),
            Cell::from("--"),
            Cell::from("--"),
        ]),
    ];

    let widths = [
        Constraint::Length(7),
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Length(8),
    ];

    let header = Row::new(vec!["Rail", "Voltage", "Current", "Power"])
        .style(Style::default().add_modifier(Modifier::BOLD));

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(" DC Rails "));
    f.render_widget(table, area);
}

fn render_power_chart(f: &mut Frame, history: &ChartHistory, scale: ChartScale, area: Rect) {
    let ac_points = history.ac_data_points();
    let dc_points = history.dc_data_points();
    let max_y = history.max_power() * 1.15;
    let x_len = CHART_HISTORY_LEN as f64;

    let (y_min, y_max) = match scale {
        ChartScale::Zero => (0.0, max_y),
        ChartScale::Auto => {
            let min = history.min_power();
            let range = max_y / 1.15 - min; // raw range without padding
            let padding = (range * 0.2).max(10.0);
            ((min - padding).max(0.0), min + range + padding)
        }
    };

    let scale_label = match scale {
        ChartScale::Zero => "0-base",
        ChartScale::Auto => "auto",
    };

    // DC drawn first (underneath), AC drawn last (on top, more important)
    let datasets = vec![
        Dataset::default()
            .name("DC Output")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Cyan))
            .data(&dc_points),
        Dataset::default()
            .name("AC Input")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Red))
            .data(&ac_points),
    ];

    // X-axis labels: left=time, center=legend, right=now
    let x_labels: Vec<Line> = vec![
        Line::from(format!("-{}s", CHART_HISTORY_LEN / 2)),
        Line::from(vec![
            Span::styled("■", Style::default().fg(Color::Red)),
            Span::raw(" AC  "),
            Span::styled("■", Style::default().fg(Color::Cyan)),
            Span::raw(" DC"),
        ]),
        Line::from("now"),
    ];
    let y_labels = vec![
        Span::raw(format!("{}W", y_min as u32)),
        Span::raw(format!("{}W", ((y_min + y_max) / 2.0) as u32)),
        Span::raw(format!("{}W", y_max as u32)),
    ];

    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Power Trend [{}] ", scale_label)),
        )
        .x_axis(Axis::default().labels(x_labels).bounds([0.0, x_len]))
        .y_axis(Axis::default().labels(y_labels).bounds([y_min, y_max]))
        .legend_position(None);

    f.render_widget(chart, area);
}

fn render_thermal_panel(f: &mut Frame, snap: &PsuSnapshot, area: Rect) {
    let temp_color = |t: f64| -> Color {
        if t > 60.0 {
            Color::Red
        } else if t > 45.0 {
            Color::Yellow
        } else {
            Color::Green
        }
    };

    let lines = vec![
        Line::from(vec![
            Span::raw("  Main:  "),
            Span::styled(
                format!("{:>5.1}", snap.thermal.temp_main_c),
                Style::default().fg(temp_color(snap.thermal.temp_main_c)),
            ),
            Span::raw(" C"),
        ]),
        Line::from(format!("  Air1:  {:>5.1} C", snap.thermal.temp_air_c)),
        Line::from(format!("  Air2:  {:>5.1} C", snap.thermal.temp_air2_c)),
        Line::from(format!("  Fan:   {:>5} RPM", snap.fan.rpm)),
        Line::from(format!("  PWM:   {:>5}", snap.fan.pwm)),
    ];

    let block = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Thermal & Fan "),
    );
    f.render_widget(block, area);
}

#[allow(clippy::too_many_arguments)]
fn render_cost_panel(
    f: &mut Frame,
    total_kwh: f64,
    total_cost: f64,
    currency: &str,
    price: f64,
    duration_s: f64,
    ac_avg_w: f64,
    area: Rect,
) {
    let hours = (duration_s / 3600.0) as u64;
    let minutes = ((duration_s % 3600.0) / 60.0) as u64;
    let secs = (duration_s % 60.0) as u64;

    // Projections based on current average power
    let daily_kwh = ac_avg_w * 24.0 / 1000.0;
    let daily_cost = daily_kwh * price;
    let weekly_cost = daily_cost * 7.0;
    let monthly_cost = daily_cost * 30.0;

    let est_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);

    let lines = vec![
        Line::from(format!("  Used: {:>8.4} kWh", total_kwh)),
        Line::from(format!("  Cost: {:>8.4} {}", total_cost, currency)),
        Line::from(format!("  Rate: {:>8.2} {}/kWh", price, currency)),
        Line::from(format!("  Time: {:>02}:{:02}:{:02}", hours, minutes, secs)),
        Line::from(vec![
            Span::raw("  /day: "),
            Span::styled(
                format!("{:>5.1} kWh {:>6.2} {}", daily_kwh, daily_cost, currency),
                est_style,
            ),
        ]),
        Line::from(vec![
            Span::raw("  /wk:  "),
            Span::styled(
                format!(
                    "{:>5.0} kWh {:>6.1} {}",
                    daily_kwh * 7.0,
                    weekly_cost,
                    currency
                ),
                est_style,
            ),
        ]),
        Line::from(vec![
            Span::raw("  /mo:  "),
            Span::styled(
                format!(
                    "{:>5.0} kWh {:>6.0} {}",
                    daily_kwh * 30.0,
                    monthly_cost,
                    currency
                ),
                est_style,
            ),
        ]),
    ];

    let block = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Cost "));
    f.render_widget(block, area);
}
