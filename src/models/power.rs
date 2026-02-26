use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Punto di potenza che ritorniamo nella nostra API
#[derive(Debug, Serialize, ToSchema)]
pub struct PowerPoint {
    pub timestamp: DateTime<Utc>,
    pub power_kw: f64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlantStatusResponse {
    pub timestamp: DateTime<Utc>,
    pub data: PlantData,
}

/// Dati completi dell'impianto
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PlantData {
    pub power_kw: f64,
    pub voltage_v: f64,
    pub current_a: f64,
    pub frequency_hz: f64,
    pub temperature_c: f64,
    pub status: u16, // 1=Running, 0=Stopped, 2=Error
    pub efficiency_percent: f64,
    pub daily_energy_kwh: f64,
    pub weather_code: u16,
    pub is_day: bool,
    pub power_factor: f64,
    pub reactive_power_kvar: f64,
    pub apparent_power_kva: f64,
}

impl Default for PlantData {
    fn default() -> Self {
        Self {
            power_kw: 0.0,
            voltage_v: 230.0,
            current_a: 0.0,
            frequency_hz: 50.0,
            temperature_c: 25.0,
            status: 1,
            efficiency_percent: 98.5,
            daily_energy_kwh: 0.0,
            weather_code: 0,
            is_day: true,
            power_factor: 1.0,
            reactive_power_kvar: 0.0,
            apparent_power_kva: 0.0,
        }
    }
}

/// Risposta current di Open-Meteo (semplificata)
#[derive(Debug, Deserialize)]
pub struct CurrentWeatherResponse {
    pub current: CurrentData,
}

#[derive(Debug, Deserialize)]
pub struct CurrentData {
    pub time: String,                   // es: "2025-12-28T10:40"
    pub shortwave_radiation: Option<f64>,
    pub temperature_2m: Option<f64>,
    pub weather_code: Option<u16>,
    pub is_day: Option<u8>,
}

#[derive(Debug)]
pub struct SimulationData {
    pub timestamp: DateTime<Utc>,
    pub power_kw: f64,
    pub temperature_c: f64,
    pub weather_code: u16,
    pub is_day: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ModbusInfo {
    pub plant_id: String,
    pub register_address: u16,
    pub length: u16,
    pub data_type: String,
    pub description: String,
}
/// Public system configuration (what the frontend needs to know)
#[derive(Debug, Serialize, ToSchema)]
pub struct SystemConfig {
    pub api_port: u16,
    pub modbus_port: u16,
    pub modbus_host: String,
}