use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

fn default_offline_mode() -> bool { false }
fn default_mqtt_topic_prefix() -> String { "solar".to_string() }
fn default_mqtt_port() -> u16 { 1883 }
fn default_mqtt_enabled() -> bool { false }

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub modbus: ModbusConfig,
    #[serde(default = "default_offline_mode")]
    pub offline_mode: bool,
    pub plants: Vec<PlantConfig>,
    #[serde(default)]
    pub mqtt: MqttConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ModbusConfig {
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MqttConfig {
    #[serde(default = "default_mqtt_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub broker_host: String,
    #[serde(default = "default_mqtt_port")]
    pub broker_port: u16,
    #[serde(default = "default_mqtt_topic_prefix")]
    pub topic_prefix: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    /// Publish interval in seconds
    #[serde(default)]
    pub publish_interval_s: Option<u64>,
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            broker_host: String::new(),
            broker_port: 1883,
            topic_prefix: "solar".to_string(),
            client_id: "solar-scada-sim".to_string(),
            username: None,
            password: None,
            publish_interval_s: None,
        }
    }
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

/// Starting Modbus register address for this plant.
/// All 27 variables are mapped at [base_address + offset] where offsets
/// are the REG_* constants in modbus_server.rs. Use ≥100-register blocks
/// between plants to avoid overlaps  (plant_1=0, plant_2=100, plant_3=200).
#[derive(Debug, Deserialize, Serialize, Clone, ToSchema)]
pub struct ModbusMapping {
    pub base_address: u16,
}

impl Config {
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config = serde_json::from_str(&content)?;
        Ok(config)
    }
}
