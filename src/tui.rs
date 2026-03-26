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
                f, &snap, total_kwh, total_cost, &currency, price, duration_s, &history,
            );
        })?;

        // Handle input
        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    _ => {}
                }
            }
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
) {
    let area = f.area();

    // Main layout: device info | middle panels | chart | bottom panels | status bar
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // device info
            Constraint::Length(9), // middle section (power + DC)
            Constraint::Min(8),    // chart
            Constraint::Length(6), // bottom section (thermal + cost)
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
    render_power_chart(f, history, main_chunks[2]);

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
        bottom_chunks[1],
    );

    // Status bar
    let status_text = format!(
        " Packets: {} | Errors: {} | Age: {:.1}s | Samples: {} | Press 'q' to exit",
        snap.meta.packet_count, snap.meta.error_count, snap.meta.data_age_s, history.sample_count,
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

fn render_power_chart(f: &mut Frame, history: &ChartHistory, area: Rect) {
    let ac_points = history.ac_data_points();
    let dc_points = history.dc_data_points();
    let max_y = history.max_power() * 1.15;
    let x_len = CHART_HISTORY_LEN as f64;

    let datasets = vec![
        Dataset::default()
            .name("AC Input")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Red))
            .data(&ac_points),
        Dataset::default()
            .name("DC Output")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Cyan))
            .data(&dc_points),
    ];

    let x_labels = vec![
        Span::raw(format!("-{}s", CHART_HISTORY_LEN / 2)),
        Span::raw("now"),
    ];
    let y_labels = vec![
        Span::raw("0W"),
        Span::raw(format!("{}W", max_y as u32 / 2)),
        Span::raw(format!("{}W", max_y as u32)),
    ];

    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Power Trend "),
        )
        .x_axis(Axis::default().labels(x_labels).bounds([0.0, x_len]))
        .y_axis(Axis::default().labels(y_labels).bounds([0.0, max_y]));

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
            Span::raw("  Main: "),
            Span::styled(
                format!("{:.1}C", snap.thermal.temp_main_c),
                Style::default().fg(temp_color(snap.thermal.temp_main_c)),
            ),
            Span::raw(format!(
                "  Air1: {:.1}C  Air2: {:.1}C",
                snap.thermal.temp_air_c, snap.thermal.temp_air2_c
            )),
        ]),
        Line::from(format!(
            "  Fan: {} RPM (PWM: {})",
            snap.fan.rpm, snap.fan.pwm
        )),
    ];

    let block = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Thermal & Fan "),
    );
    f.render_widget(block, area);
}

fn render_cost_panel(
    f: &mut Frame,
    total_kwh: f64,
    total_cost: f64,
    currency: &str,
    price: f64,
    duration_s: f64,
    area: Rect,
) {
    let hours = (duration_s / 3600.0) as u64;
    let minutes = ((duration_s % 3600.0) / 60.0) as u64;
    let secs = (duration_s % 60.0) as u64;

    let lines = vec![
        Line::from(format!(
            "  {:.4} kWh | {:.4} {}",
            total_kwh, total_cost, currency
        )),
        Line::from(format!(
            "  {:.2} {}/kWh | {:02}:{:02}:{:02}",
            price, currency, hours, minutes, secs
        )),
    ];

    let block = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Cost "));
    f.render_widget(block, area);
}
