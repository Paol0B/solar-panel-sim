use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// ─── Core plant status ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, ToSchema)]
pub struct PlantStatusResponse {
    pub timestamp: DateTime<Utc>,
    pub data: PlantData,
}

/// Complete inverter telemetry — mirrors a real grid-tied inverter data model.
/// Covers DC input (MPPT), 3-phase AC output, grid protection, thermal and
/// energy accounting parameters.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PlantData {
    // ── AC Output (3-phase) ──────────────────────────────────────────────────
    /// Total AC active power output (kW)
    pub power_kw: f64,
    /// L1/L2/L3 phase voltages (V)
    pub voltage_l1_v: f64,
    pub voltage_l2_v: f64,
    pub voltage_l3_v: f64,
    /// L1/L2/L3 phase currents (A)
    pub current_l1_a: f64,
    pub current_l2_a: f64,
    pub current_l3_a: f64,
    /// Grid frequency (Hz)
    pub frequency_hz: f64,
    /// Rate of Change of Frequency (Hz/s) — grid protection
    pub rocof_hz_s: f64,
    /// Total power factor (cos φ)
    pub power_factor: f64,
    /// Reactive power (kVAr)
    pub reactive_power_kvar: f64,
    /// Apparent power (kVA)
    pub apparent_power_kva: f64,

    // ── DC Input / MPPT ─────────────────────────────────────────────────────
    /// DC bus voltage from panels (V)
    pub dc_voltage_v: f64,
    /// DC input current (A)
    pub dc_current_a: f64,
    /// DC power from panels (kW) — before inverter conversion
    pub dc_power_kw: f64,
    /// MPPT tracker operating voltage (V_mpp)
    pub mppt_voltage_v: f64,
    /// MPPT tracker operating current (A_mpp)
    pub mppt_current_a: f64,

    // ── Thermal ──────────────────────────────────────────────────────────────
    /// Panel/cell temperature (°C)
    pub temperature_c: f64,
    /// Inverter heatsink internal temperature (°C)
    pub inverter_temp_c: f64,
    /// Ambient temperature at plant site (°C)
    pub ambient_temp_c: f64,

    // ── Inverter metrics ─────────────────────────────────────────────────────
    /// Inverter AC conversion efficiency (%)
    pub efficiency_percent: f64,
    /// Plane-of-Array irradiance (W/m²)
    pub poa_irradiance_w_m2: f64,
    /// Solar elevation angle (deg)
    pub solar_elevation_deg: f64,
    /// Cloud attenuation factor [0..1]
    pub cloud_factor: f64,

    // ── Safety / Grid protection ─────────────────────────────────────────────
    /// Isolation resistance DC-ground (MΩ) — IEC 62109: must be >1 MΩ
    pub isolation_resistance_mohm: f64,
    /// Status: 0=Stopped, 1=Running, 2=Fault, 3=Curtailed, 4=Starting, 5=MPPT
    pub status: u16,
    /// Active IEC/VDE fault code (0 = no fault)
    pub fault_code: u16,
    /// Bitmask of active alarm flags
    pub alarm_flags: u32,

    // ── Weather ───────────────────────────────────────────────────────────────
    pub weather_code: u16,
    pub is_day: bool,

    // ── Energy counters ───────────────────────────────────────────────────────
    /// Energy produced today (kWh)
    pub daily_energy_kwh: f64,
    /// Energy produced this month (kWh)
    pub monthly_energy_kwh: f64,
    /// Total lifetime energy produced (kWh)
    pub total_energy_kwh: f64,

    // ── Performance KPIs ──────────────────────────────────────────────────────
    /// Performance Ratio = AC yield / theoretical yield (IEC 61724)
    pub performance_ratio: f64,
    /// Specific yield = daily kWh / kWp
    pub specific_yield_kwh_kwp: f64,
    /// Capacity factor (%)
    pub capacity_factor_percent: f64,
}

impl Default for PlantData {
    fn default() -> Self {
        Self {
            power_kw: 0.0,
            voltage_l1_v: 230.0,
            voltage_l2_v: 230.0,
            voltage_l3_v: 230.0,
            current_l1_a: 0.0,
            current_l2_a: 0.0,
            current_l3_a: 0.0,
            frequency_hz: 50.0,
            rocof_hz_s: 0.0,
            power_factor: 1.0,
            reactive_power_kvar: 0.0,
            apparent_power_kva: 0.0,
            dc_voltage_v: 600.0,
            dc_current_a: 0.0,
            dc_power_kw: 0.0,
            mppt_voltage_v: 600.0,
            mppt_current_a: 0.0,
            temperature_c: 25.0,
            inverter_temp_c: 35.0,
            ambient_temp_c: 20.0,
            efficiency_percent: 0.0,
            poa_irradiance_w_m2: 0.0,
            solar_elevation_deg: 0.0,
            cloud_factor: 1.0,
            isolation_resistance_mohm: 10.0,
            status: 0,
            fault_code: 0,
            alarm_flags: 0,
            weather_code: 0,
            is_day: false,
            daily_energy_kwh: 0.0,
            monthly_energy_kwh: 0.0,
            total_energy_kwh: 0.0,
            performance_ratio: 0.0,
            specific_yield_kwh_kwp: 0.0,
            capacity_factor_percent: 0.0,
        }
    }
}

// ─── Alarm / Event system ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AlarmSeverity {
    Info,
    Warning,
    Critical,
    Fault,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Alarm {
    pub id: String,
    pub plant_id: String,
    pub code: u16,
    pub severity: AlarmSeverity,
    pub message: String,
    pub timestamp: DateTime<Utc>,
    pub active: bool,
    pub cleared_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventKind {
    PlantStartup,
    PlantShutdown,
    ModeChange,
    AlarmRaised,
    AlarmCleared,
    FaultTrip,
    GridDisconnect,
    GridReconnect,
    CurtailmentStart,
    CurtailmentEnd,
    SettingChanged,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Event {
    pub id: String,
    pub plant_id: Option<String>,
    pub kind: EventKind,
    pub message: String,
    pub timestamp: DateTime<Utc>,
    pub payload: Option<serde_json::Value>,
}

// ─── Alarm codes (IEC 62116 / VDE 0126 inspired) ─────────────────────────────

pub mod alarm_codes {
    pub const NONE: u16                 = 0;
    pub const AC_OVERVOLTAGE: u16       = 101;
    pub const AC_UNDERVOLTAGE: u16      = 102;
    pub const AC_OVERFREQUENCY: u16     = 103;
    pub const AC_UNDERFREQUENCY: u16    = 104;
    pub const ROCOF_TRIP: u16           = 105;
    pub const GRID_ISLAND_DETECTED: u16 = 106;
    pub const DC_OVERVOLTAGE: u16       = 201;
    pub const DC_UNDERVOLTAGE: u16      = 202;
    pub const MPPT_FAILURE: u16         = 203;
    pub const ISOLATION_FAULT: u16      = 301;
    pub const GROUND_FAULT: u16         = 302;
    pub const OVERTEMPERATURE: u16      = 401;
    pub const FAN_FAULT: u16            = 402;
    pub const COMMUNICATION_LOSS: u16   = 501;
    pub const INTERNAL_FAULT: u16       = 999;
}

pub mod alarm_flag_bits {
    pub const AC_OVERVOLTAGE: u32      = 1 << 0;
    pub const AC_UNDERVOLTAGE: u32     = 1 << 1;
    pub const FREQUENCY_FAULT: u32     = 1 << 2;
    pub const ISOLATION_FAULT: u32     = 1 << 3;
    pub const OVERTEMPERATURE: u32     = 1 << 4;
    pub const MPPT_DEVIATION: u32      = 1 << 5;
    pub const GRID_DISCONNECT: u32     = 1 << 6;
    pub const COMMUNICATION_LOSS: u32  = 1 << 7;
}

// ─── Open-Meteo wire types ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CurrentWeatherResponse {
    pub current: CurrentData,
}

#[derive(Debug, Deserialize)]
pub struct CurrentData {
    pub time: String,
    pub shortwave_radiation: Option<f64>,
    pub temperature_2m: Option<f64>,
    pub weather_code: Option<u16>,
    pub is_day: Option<u8>,
}

// ─── Internal simulation data ────────────────────────────────────────────────

#[derive(Debug)]
pub struct SimulationData {
    pub timestamp: DateTime<Utc>,
    pub power_kw: f64,
    pub temperature_c: f64,
    pub ambient_temp_c: f64,
    pub weather_code: u16,
    pub is_day: bool,
    pub poa_irradiance_w_m2: f64,
    pub cloud_factor: f64,
    pub solar_elevation_deg: f64,
}

// ─── REST API response types ──────────────────────────────────────────────────

#[derive(Debug, Serialize, ToSchema)]
pub struct ModbusInfo {
    pub plant_id: String,
    pub register_address: u16,
    pub length: u16,
    pub data_type: String,
    pub description: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SystemConfig {
    pub api_port: u16,
    pub modbus_port: u16,
    pub modbus_host: String,
    pub mqtt_enabled: bool,
    pub mqtt_broker: Option<String>,
    pub mqtt_topic_prefix: String,
    pub websocket_endpoint: String,
    pub prometheus_endpoint: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct HealthStatus {
    pub status: String,
    pub version: String,
    pub uptime_seconds: u64,
    pub plants_online: usize,
    pub plants_total: usize,
    pub offline_mode: bool,
    pub mqtt_connected: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GlobalPowerResponse {
    pub total_power_kw: f64,
    pub total_nominal_kw: f64,
    pub total_daily_energy_kwh: f64,
    pub total_monthly_energy_kwh: f64,
    pub total_lifetime_energy_kwh: f64,
    pub fleet_performance_ratio: f64,
    pub plants_running: usize,
    pub plants_total: usize,
    pub per_plant: std::collections::HashMap<String, f64>,
}
