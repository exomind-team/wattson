use std::path::Path;

use chrono::{DateTime, Utc};
use plotters::prelude::*;
use serde::Deserialize;

use crate::config::Config;

/// A single data point for chart rendering
#[derive(Debug, Clone, Deserialize)]
pub struct DataPoint {
    pub timestamp: DateTime<Utc>,
    pub ac_input_w: f64,
    pub dc_output_w: f64,
    pub efficiency_pct: f64,
    pub temp_main_c: f64,
}

/// Parse data points from a JSON-lines file (one PsuSnapshot per line)
pub fn load_data_points(input: &Path) -> Result<Vec<DataPoint>, String> {
    let content = std::fs::read_to_string(input)
        .map_err(|e| format!("Failed to read {}: {}", input.display(), e))?;

    let mut points = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value =
            serde_json::from_str(line).map_err(|e| format!("Line {}: {}", i + 1, e))?;

        // Try to extract a timestamp; if missing, use index-based time
        let ts = v
            .get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
            .unwrap_or_else(|| {
                Utc::now() - chrono::Duration::seconds((content.lines().count() - i) as i64)
            });

        let ac = v
            .pointer("/power/ac_input_w")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0);
        let dc = v
            .pointer("/power/dc_output_est_w")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0);
        let eff = v
            .pointer("/power/efficiency_pct")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0);
        let temp = v
            .pointer("/thermal/temp_main_c")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0);

        points.push(DataPoint {
            timestamp: ts,
            ac_input_w: ac,
            dc_output_w: dc,
            efficiency_pct: eff,
            temp_main_c: temp,
        });
    }

    if points.is_empty() {
        return Err("No valid data points found".to_string());
    }

    Ok(points)
}

/// Generate a 3-panel chart (power / efficiency / temperature) and save as PNG
pub fn generate_chart(
    data: &[DataPoint],
    output: &Path,
    config: &Config,
    device_model: &str,
) -> Result<(), String> {
    if data.is_empty() {
        return Err("No data to chart".to_string());
    }

    let root = BitMapBackend::new(output, (1200, 900)).into_drawing_area();
    root.fill(&WHITE)
        .map_err(|e| format!("Drawing error: {}", e))?;

    let n = data.len();
    let x_range = 0f64..(n as f64);

    // Find ranges for each metric
    let max_power = data
        .iter()
        .map(|d| d.ac_input_w.max(d.dc_output_w))
        .fold(0.0f64, f64::max)
        * 1.1;
    let max_power = if max_power < 10.0 { 100.0 } else { max_power };

    let (eff_min, eff_max) = data.iter().fold((100.0f64, 0.0f64), |(mn, mx), d| {
        (mn.min(d.efficiency_pct), mx.max(d.efficiency_pct))
    });
    let eff_range = (eff_min - 5.0).max(0.0)..(eff_max + 5.0).min(100.0);

    let max_temp = data.iter().map(|d| d.temp_main_c).fold(0.0f64, f64::max) * 1.2;
    let max_temp = if max_temp < 10.0 { 60.0 } else { max_temp };

    // Split into 3 vertical panels
    let panels = root.split_evenly((3, 1));

    // Panel 1: Power (AC + DC)
    {
        let mut chart = ChartBuilder::on(&panels[0])
            .caption("Power (W)", ("sans-serif", 18))
            .margin(10)
            .x_label_area_size(30)
            .y_label_area_size(60)
            .build_cartesian_2d(x_range.clone(), 0.0..max_power)
            .map_err(|e| format!("Chart build error: {}", e))?;

        chart
            .configure_mesh()
            .x_desc("Sample")
            .y_desc("Watts")
            .draw()
            .map_err(|e| format!("Mesh error: {}", e))?;

        chart
            .draw_series(LineSeries::new(
                data.iter()
                    .enumerate()
                    .map(|(i, d)| (i as f64, d.ac_input_w)),
                ShapeStyle::from(&RED).stroke_width(2),
            ))
            .map_err(|e| format!("Series error: {}", e))?
            .label("AC Input")
            .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], RED));

        chart
            .draw_series(LineSeries::new(
                data.iter()
                    .enumerate()
                    .map(|(i, d)| (i as f64, d.dc_output_w)),
                ShapeStyle::from(&BLUE).stroke_width(2),
            ))
            .map_err(|e| format!("Series error: {}", e))?
            .label("DC Output")
            .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], BLUE));

        chart
            .configure_series_labels()
            .border_style(BLACK)
            .draw()
            .map_err(|e| format!("Legend error: {}", e))?;
    }

    // Panel 2: Efficiency
    {
        let mut chart = ChartBuilder::on(&panels[1])
            .caption("Efficiency (%)", ("sans-serif", 18))
            .margin(10)
            .x_label_area_size(30)
            .y_label_area_size(60)
            .build_cartesian_2d(x_range.clone(), eff_range)
            .map_err(|e| format!("Chart build error: {}", e))?;

        chart
            .configure_mesh()
            .x_desc("Sample")
            .y_desc("%")
            .draw()
            .map_err(|e| format!("Mesh error: {}", e))?;

        chart
            .draw_series(LineSeries::new(
                data.iter()
                    .enumerate()
                    .map(|(i, d)| (i as f64, d.efficiency_pct)),
                ShapeStyle::from(&GREEN).stroke_width(2),
            ))
            .map_err(|e| format!("Series error: {}", e))?;
    }

    // Panel 3: Temperature
    {
        let mut chart = ChartBuilder::on(&panels[2])
            .caption("Temperature (C)", ("sans-serif", 18))
            .margin(10)
            .x_label_area_size(30)
            .y_label_area_size(60)
            .build_cartesian_2d(x_range, 0.0..max_temp)
            .map_err(|e| format!("Chart build error: {}", e))?;

        chart
            .configure_mesh()
            .x_desc("Sample")
            .y_desc("Celsius")
            .draw()
            .map_err(|e| format!("Mesh error: {}", e))?;

        chart
            .draw_series(LineSeries::new(
                data.iter()
                    .enumerate()
                    .map(|(i, d)| (i as f64, d.temp_main_c)),
                ShapeStyle::from(&Palette99::pick(3)).stroke_width(2),
            ))
            .map_err(|e| format!("Series error: {}", e))?;
    }

    // Draw watermark at the bottom
    let watermark = format!(
        "{} | {}",
        config.chart.watermark,
        if device_model.is_empty() {
            "Unknown PSU"
        } else {
            device_model
        }
    );
    root.draw_text(
        &watermark,
        &TextStyle::from(("sans-serif", 12).into_font()).color(&RGBColor(180, 180, 180)),
        (20, 880),
    )
    .map_err(|e| format!("Watermark error: {}", e))?;

    root.present()
        .map_err(|e| format!("Failed to save chart: {}", e))?;

    Ok(())
}
