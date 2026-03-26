use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::State;
use axum::http::Method;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use tower_http::cors::{Any, CorsLayer};

use crate::config::Config;
use crate::serial::PsuHandle;

/// Shared application state for the API server
struct AppState {
    handle: PsuHandle,
    cost_wh: Mutex<f64>,
    start_time: Instant,
    last_sample: Mutex<Instant>,
    price_per_kwh: f64,
    currency: String,
}

/// Health check response
#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    connected: bool,
}

/// Cost response
#[derive(Serialize)]
struct CostResponse {
    total_kwh: f64,
    total_cost: f64,
    currency: String,
    price_per_kwh: f64,
    monitoring_duration_s: f64,
}

/// Start the API server (blocking — runs on tokio runtime)
pub async fn serve(handle: PsuHandle, config: &Config) {
    let state = Arc::new(AppState {
        handle,
        cost_wh: Mutex::new(0.0),
        start_time: Instant::now(),
        last_sample: Mutex::new(Instant::now()),
        price_per_kwh: config.cost.price_per_kwh,
        currency: config.cost.currency.clone(),
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET])
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(get_status))
        .route("/api/status", get(get_status))
        .route("/api/power", get(get_power))
        .route("/api/voltage", get(get_voltage))
        .route("/api/temperature", get(get_temperature))
        .route("/api/device", get(get_device))
        .route("/api/cost", get(get_cost))
        .route("/health", get(health_check))
        .layer(cors)
        .with_state(state);

    let addr = format!("0.0.0.0:{}", config.api.port);
    eprintln!("API server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind API server");
    axum::serve(listener, app)
        .await
        .expect("API server error");
}

async fn get_status(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    update_cost(&state);
    let snap = state.handle.latest();
    Json(serde_json::to_value(&snap).unwrap())
}

async fn get_power(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let snap = state.handle.latest();
    Json(serde_json::json!({
        "ac_input_w": snap.power.ac_input_w,
        "ac_input_avg_w": snap.power.ac_input_avg_w,
        "dc_output_est_w": snap.power.dc_output_est_w,
        "efficiency_pct": snap.power.efficiency_pct,
    }))
}

async fn get_voltage(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let snap = state.handle.latest();
    Json(serde_json::json!({
        "ac": {
            "voltage_v": snap.ac.voltage_v,
            "frequency_hz": snap.ac.frequency_hz,
        },
        "dc": {
            "volt_12v": snap.dc.volt_12v,
            "volt_5v": snap.dc.volt_5v,
            "volt_3v3": snap.dc.volt_3v3,
            "volt_5vsb": snap.dc.volt_5vsb,
        }
    }))
}

async fn get_temperature(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let snap = state.handle.latest();
    Json(serde_json::json!({
        "temp_main_c": snap.thermal.temp_main_c,
        "temp_air_c": snap.thermal.temp_air_c,
        "temp_air2_c": snap.thermal.temp_air2_c,
        "fan_rpm": snap.fan.rpm,
        "fan_pwm": snap.fan.pwm,
    }))
}

async fn get_device(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let snap = state.handle.latest();
    Json(serde_json::json!({
        "model": snap.device.model,
        "serial": snap.device.serial,
        "connected": snap.meta.connected,
        "packet_count": snap.meta.packet_count,
        "error_count": snap.meta.error_count,
    }))
}

async fn get_cost(State(state): State<Arc<AppState>>) -> Json<CostResponse> {
    update_cost(&state);
    let total_wh = *state.cost_wh.lock().unwrap();
    let total_kwh = total_wh / 1000.0;
    Json(CostResponse {
        total_kwh,
        total_cost: total_kwh * state.price_per_kwh,
        currency: state.currency.clone(),
        price_per_kwh: state.price_per_kwh,
        monitoring_duration_s: state.start_time.elapsed().as_secs_f64(),
    })
}

async fn health_check(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        connected: state.handle.is_connected(),
    })
}

/// Update cost accumulator based on current power reading
fn update_cost(state: &AppState) {
    let snap = state.handle.latest();
    let mut last = state.last_sample.lock().unwrap();
    let elapsed_h = last.elapsed().as_secs_f64() / 3600.0;
    let mut wh = state.cost_wh.lock().unwrap();
    *wh += snap.power.ac_input_w * elapsed_h;
    *last = Instant::now();
}
