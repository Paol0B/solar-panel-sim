use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

use crate::config::{Config, PlantConfig};
use crate::models::power::{ModbusInfo, PlantStatusResponse};
use crate::shared_state::AppState;

/// GET /api/plants
/// List all configured plants
/// 
/// Returns a list of all solar plants configured in the system, including their ID, location, capacity, and Modbus register address.
#[utoipa::path(
    get,
    path = "/api/plants",
    responses(
        (status = 200, description = "List of configured plants", body = Vec<PlantConfig>),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_plants(State(config): State<Config>) -> impl IntoResponse {
    Json(config.plants).into_response()
}

/// GET /api/plants/{id}/power
/// Get current status for a specific plant
/// 
/// Returns the latest complete status (Power, Voltage, Current, etc.) for the specified plant.
/// This value is updated periodically (every minute) in the background.
#[utoipa::path(
    get,
    path = "/api/plants/{id}/power",
    params(
        ("id" = String, Path, description = "Unique Plant ID")
    ),
    responses(
        (status = 200, description = "Current plant status", body = PlantStatusResponse),
        (status = 404, description = "Plant not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_plant_power(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if let Some(data) = state.get_data(&id) {
        let response = PlantStatusResponse {
            timestamp: chrono::Utc::now(),
            data,
        };
        (StatusCode::OK, Json(response)).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Plant not found"}))).into_response()
    }
}

/// GET /api/power/global
/// Get current power for all plants
/// 
/// Returns a map where keys are plant IDs and values are current power in kW.
#[utoipa::path(
    get,
    path = "/api/power/global",
    responses(
        (status = 200, description = "Map of plant ID to current power", body = HashMap<String, f64>),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_global_power(State(state): State<AppState>) -> impl IntoResponse {
    let all_data = state.get_all_data();
    let powers: std::collections::HashMap<String, f64> = all_data.into_iter()
        .map(|(k, v)| (k, v.power_kw))
        .collect();
    Json(powers).into_response()
}

/// GET /api/modbus/info
/// Get Modbus register information
/// 
/// Returns a list of Modbus registers available for reading, including address, length, and data type.
#[utoipa::path(
    get,
    path = "/api/modbus/info",
    responses(
        (status = 200, description = "List of Modbus registers", body = Vec<ModbusInfo>),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_modbus_info(State(config): State<Config>) -> impl IntoResponse {
    // All numeric variables are encoded as IEEE 754 float32 split across
    // TWO consecutive u16 registers (big-endian: high word first).
    // Status uses a single u16 register (raw value, no encoding).
    let mut info = Vec::new();
    for p in &config.plants {
        info.push(ModbusInfo {
            plant_id: p.id.clone(),
            register_address: p.modbus_mapping.power_address,
            length: 2,
            data_type: "float32 IEEE 754 (2 regs)".to_string(),
            description: format!("Power output for {} in kW — no upper limit", p.name),
        });
        info.push(ModbusInfo {
            plant_id: p.id.clone(),
            register_address: p.modbus_mapping.voltage_address,
            length: 2,
            data_type: "float32 IEEE 754 (2 regs)".to_string(),
            description: format!("AC Voltage for {} in V", p.name),
        });
        info.push(ModbusInfo {
            plant_id: p.id.clone(),
            register_address: p.modbus_mapping.current_address,
            length: 2,
            data_type: "float32 IEEE 754 (2 regs)".to_string(),
            description: format!("AC Current for {} in A", p.name),
        });
        info.push(ModbusInfo {
            plant_id: p.id.clone(),
            register_address: p.modbus_mapping.frequency_address,
            length: 2,
            data_type: "float32 IEEE 754 (2 regs)".to_string(),
            description: format!("Grid frequency for {} in Hz", p.name),
        });
        info.push(ModbusInfo {
            plant_id: p.id.clone(),
            register_address: p.modbus_mapping.temperature_address,
            length: 2,
            data_type: "float32 IEEE 754 (2 regs)".to_string(),
            description: format!("Panel cell temperature for {} in °C", p.name),
        });
        info.push(ModbusInfo {
            plant_id: p.id.clone(),
            register_address: p.modbus_mapping.status_address,
            length: 1,
            data_type: "u16 (raw)".to_string(),
            description: format!("Status for {} (1=Running, 0=Stopped)", p.name),
        });
    }
    Json(info).into_response()
}
