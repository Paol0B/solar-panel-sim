use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

fn default_offline_mode() -> bool { false }

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub modbus: ModbusConfig,
    #[serde(default = "default_offline_mode")]
    pub offline_mode: bool,
    pub plants: Vec<PlantConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ModbusConfig {
    pub port: u16,
}

#[derive(Debug, Deserialize, Serialize, Clone, ToSchema)]
pub struct PlantConfig {
    pub id: String,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub nominal_power_kw: f64,
    pub timezone: String,
    pub modbus_mapping: ModbusMapping,
}

#[derive(Debug, Deserialize, Serialize, Clone, ToSchema)]
pub struct ModbusMapping {
    pub power_address: u16,
    pub voltage_address: u16,
    pub current_address: u16,
    pub frequency_address: u16,
    pub temperature_address: u16,
    pub status_address: u16,
}

impl Config {
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config = serde_json::from_str(&content)?;
        Ok(config)
    }
}
