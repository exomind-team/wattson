use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::State;
use axum::http::{Method, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};

use crate::config::Config;
use crate::error::WattsonError;
use crate::protocol::FanMode;
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

#[derive(Debug, Deserialize)]
struct FanSpeedRequest {
    pwm: u8,
}

#[derive(Debug, Deserialize)]
struct FanCurveRequest {
    points: Vec<(u8, u8)>,
}

#[derive(Debug, Deserialize)]
struct FanModeRequest {
    mode: FanMode,
}

#[derive(Debug, Serialize)]
struct CommandResponse {
    ok: bool,
    action: &'static str,
    message: String,
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
        .allow_methods([Method::GET, Method::POST])
        .allow_headers(Any);

    let app = build_router(state).layer(cors);

    let addr = format!("0.0.0.0:{}", config.api.port);
    eprintln!("API server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind API server");
    axum::serve(listener, app).await.expect("API server error");
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

async fn set_fan_speed(
    State(state): State<Arc<AppState>>,
    Json(request): Json<FanSpeedRequest>,
) -> Result<Json<CommandResponse>, (StatusCode, Json<serde_json::Value>)> {
    if request.pwm > 100 {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "fan pwm must be within 0..=100 / 风扇占空比必须在 0..=100",
        ));
    }

    state
        .handle
        .set_fan_pwm(request.pwm)
        .map_err(map_write_error)?;

    Ok(Json(CommandResponse {
        ok: true,
        action: "fan_speed",
        message: format!(
            "Fan PWM set to {} (风扇占空比已设置为 {})",
            request.pwm, request.pwm
        ),
    }))
}

async fn set_fan_curve(
    State(state): State<Arc<AppState>>,
    Json(request): Json<FanCurveRequest>,
) -> Result<Json<CommandResponse>, (StatusCode, Json<serde_json::Value>)> {
    state
        .handle
        .set_fan_curve(request.points.clone())
        .map_err(map_write_error)?;

    Ok(Json(CommandResponse {
        ok: true,
        action: "fan_curve",
        message: format!(
            "Fan curve applied (已应用风扇曲线), points={}",
            request.points.len()
        ),
    }))
}

async fn set_fan_mode(
    State(state): State<Arc<AppState>>,
    Json(request): Json<FanModeRequest>,
) -> Result<Json<CommandResponse>, (StatusCode, Json<serde_json::Value>)> {
    state
        .handle
        .set_fan_mode(request.mode)
        .map_err(map_write_error)?;

    Ok(Json(CommandResponse {
        ok: true,
        action: "fan_mode",
        message: format!(
            "Fan mode set to {} (风扇模式已设置为 {})",
            request.mode,
            request.mode.label()
        ),
    }))
}

fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(get_status))
        .route("/api/status", get(get_status))
        .route("/api/power", get(get_power))
        .route("/api/voltage", get(get_voltage))
        .route("/api/temperature", get(get_temperature))
        .route("/api/device", get(get_device))
        .route("/api/cost", get(get_cost))
        .route("/api/fan/speed", post(set_fan_speed))
        .route("/api/fan/curve", post(set_fan_curve))
        .route("/api/fan/mode", post(set_fan_mode))
        .route("/health", get(health_check))
        .with_state(state)
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

fn map_write_error(error: WattsonError) -> (StatusCode, Json<serde_json::Value>) {
    match error {
        WattsonError::Protocol { message } | WattsonError::Config { message } => {
            api_error(StatusCode::BAD_REQUEST, &message)
        }
        WattsonError::NotConnected => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "device not connected / 设备未连接",
        ),
        WattsonError::Timeout => api_error(
            StatusCode::GATEWAY_TIMEOUT,
            "serial command timed out / 串口命令超时",
        ),
        other => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("serial write failed / 串口写入失败: {other}"),
        ),
    }
}

fn api_error(status: StatusCode, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "ok": false,
            "message": message,
        })),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use tower::ServiceExt;

    use super::*;
    use crate::data::PsuSnapshot;
    use crate::protocol::FanMode;
    use crate::serial::{test_handle_with_recorder, SerialCommand};

    fn test_state() -> (Arc<AppState>, std::sync::mpsc::Receiver<SerialCommand>) {
        let mut snapshot = PsuSnapshot::default();
        snapshot.meta.connected = true;
        let (handle, receiver) = test_handle_with_recorder(snapshot);
        let state = Arc::new(AppState {
            handle,
            cost_wh: Mutex::new(0.0),
            start_time: Instant::now(),
            last_sample: Mutex::new(Instant::now()),
            price_per_kwh: 0.56,
            currency: "CNY".to_string(),
        });
        (state, receiver)
    }

    #[tokio::test]
    async fn post_fan_speed_enqueues_pwm_command() {
        let (state, receiver) = test_state();
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/fan/speed")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"pwm":55}"#))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("recorded command"),
            SerialCommand::Pwm(55)
        );
    }

    #[tokio::test]
    async fn post_fan_curve_enqueues_curve_command() {
        let (state, receiver) = test_state();
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/fan/curve")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"points":[[30,40],[50,55],[70,75]]}"#))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("recorded command"),
            SerialCommand::Curve(vec![(30, 40), (50, 55), (70, 75)])
        );
    }

    #[tokio::test]
    async fn post_fan_mode_enqueues_mode_command() {
        let (state, receiver) = test_state();
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/fan/mode")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"mode":"custom"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("recorded command"),
            SerialCommand::Mode(FanMode::Custom)
        );
    }
}
