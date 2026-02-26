use axum::{
    extract::{Path, Query, State, WebSocketUpgrade},
    extract::ws::{Message, WebSocket},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;

use crate::config::{Config, PlantConfig};
use crate::models::power::{
    Alarm, Event, GlobalPowerResponse, HealthStatus, ModbusInfo, PlantStatusResponse, SystemConfig,
};
use crate::shared_state::AppState;

// ─── Plants ──────────────────────────────────────────────────────────────────

/// GET /api/plants
#[utoipa::path(get, path = "/api/plants",
    responses((status = 200, description = "List of configured plants", body = Vec<PlantConfig>)))]
pub async fn list_plants(State(config): State<Config>) -> impl IntoResponse {
    Json(config.plants).into_response()
}

// ─── Plant telemetry ──────────────────────────────────────────────────────────

/// GET /api/plants/{id}/power
#[utoipa::path(get, path = "/api/plants/{id}/power",
    params(("id" = String, Path, description = "Plant ID")),
    responses(
        (status = 200, description = "Current plant status", body = PlantStatusResponse),
        (status = 404, description = "Plant not found")
    ))]
pub async fn get_plant_power(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if let Some(data) = state.get_data(&id) {
        (StatusCode::OK, Json(PlantStatusResponse { timestamp: chrono::Utc::now(), data })).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Plant not found"}))).into_response()
    }
}

// ─── Global fleet summary ────────────────────────────────────────────────────

/// GET /api/power/global
#[utoipa::path(get, path = "/api/power/global",
    responses((status = 200, description = "Fleet summary", body = GlobalPowerResponse)))]
pub async fn get_global_power(
    State(state): State<AppState>,
    State(config): State<Config>,
) -> impl IntoResponse {
    let all_data  = state.get_all_data();
    let total_nom : f64 = config.plants.iter().map(|p| p.nominal_power_kw).sum();

    let total_power   = all_data.values().map(|d| d.power_kw).sum::<f64>();
    let total_daily   = all_data.values().map(|d| d.daily_energy_kwh).sum::<f64>();
    let total_monthly = all_data.values().map(|d| d.monthly_energy_kwh).sum::<f64>();
    let total_life    = all_data.values().map(|d| d.total_energy_kwh).sum::<f64>();
    let running       = all_data.values().filter(|d| d.status == 1 || d.status == 5).count();
    let fleet_pr      = if !all_data.is_empty() {
        all_data.values().map(|d| d.performance_ratio).sum::<f64>() / all_data.len() as f64
    } else { 0.0 };
    let per_plant = all_data.into_iter().map(|(k, v)| (k, v.power_kw)).collect();

    Json(GlobalPowerResponse {
        total_power_kw:             total_power,
        total_nominal_kw:           total_nom,
        total_daily_energy_kwh:     total_daily,
        total_monthly_energy_kwh:   total_monthly,
        total_lifetime_energy_kwh:  total_life,
        fleet_performance_ratio:    fleet_pr,
        plants_running:             running,
        plants_total:               config.plants.len(),
        per_plant,
    })
}

// ─── Modbus register info ────────────────────────────────────────────────────

/// GET /api/modbus/info
#[utoipa::path(get, path = "/api/modbus/info",
    responses((status = 200, description = "Modbus register map", body = Vec<ModbusInfo>)))]
pub async fn get_modbus_info(State(config): State<Config>) -> impl IntoResponse {
    use crate::modbus_server::*;
    // Static register layout: (offset, regs, data_type, description, unit)
    // Offsets are the REG_* constants from modbus_server.rs.
    const LAYOUT: &[(u16, u16, &str, &str, &str)] = &[
        // AC Output
        (REG_POWER_KW,            2, "float32 IE754", "Active power",                 "kW"),
        (REG_VOLTAGE_L1_V,        2, "float32 IE754", "AC Voltage L1",                "V"),
        (REG_CURRENT_L1_A,        2, "float32 IE754", "AC Current L1",                "A"),
        (REG_FREQUENCY_HZ,        2, "float32 IE754", "Grid frequency",               "Hz"),
        (REG_TEMPERATURE_C,       2, "float32 IE754", "Cell temperature",             "°C"),
        (REG_STATUS,              1, "u16 raw",        "Inverter status (enum 0-5)",   "—"),
        (REG_VOLTAGE_L2_V,        2, "float32 IE754", "AC Voltage L2",                "V"),
        (REG_VOLTAGE_L3_V,        2, "float32 IE754", "AC Voltage L3",                "V"),
        (REG_CURRENT_L2_A,        2, "float32 IE754", "AC Current L2",                "A"),
        (REG_CURRENT_L3_A,        2, "float32 IE754", "AC Current L3",                "A"),
        (REG_REACTIVE_POWER_KVAR, 2, "float32 IE754", "Reactive power Q",             "kvar"),
        (REG_APPARENT_POWER_KVA,  2, "float32 IE754", "Apparent power S",             "kVA"),
        (REG_POWER_FACTOR,        2, "float32 IE754", "Power factor cos φ",           "—"),
        (REG_ROCOF_HZ_S,          2, "float32 IE754", "ROCOF (df/dt)",                "Hz/s"),
        // DC / MPPT
        (REG_DC_VOLTAGE_V,        2, "float32 IE754", "DC link voltage",              "V"),
        (REG_DC_CURRENT_A,        2, "float32 IE754", "DC string current",            "A"),
        (REG_DC_POWER_KW,         2, "float32 IE754", "DC input power",               "kW"),
        (REG_MPPT_VOLTAGE_V,      2, "float32 IE754", "MPPT operating voltage",       "V"),
        (REG_MPPT_CURRENT_A,      2, "float32 IE754", "MPPT operating current",       "A"),
        // Thermal
        (REG_INVERTER_TEMP_C,     2, "float32 IE754", "Inverter heatsink temperature","°C"),
        (REG_AMBIENT_TEMP_C,      2, "float32 IE754", "Ambient temperature",          "°C"),
        // Performance & Irradiance
        (REG_EFFICIENCY_PCT,      2, "float32 IE754", "Inverter efficiency",          "%"),
        (REG_POA_IRRADIANCE,      2, "float32 IE754", "Plane-of-Array irradiance",    "W/m²"),
        (REG_SOLAR_ELEVATION,     2, "float32 IE754", "Solar elevation angle",        "°"),
        (REG_PERF_RATIO,          2, "float32 IE754", "Performance Ratio (IEC 61724)","—"),
        (REG_SPECIFIC_YIELD,      2, "float32 IE754", "Specific yield",               "kWh/kWp"),
        (REG_CAPACITY_FACTOR,     2, "float32 IE754", "Capacity factor",              "%"),
        // Safety & Alarms
        (REG_ISOLATION_MOHM,      2, "float32 IE754", "Isolation resistance DC-GND",  "MΩ"),
        (REG_FAULT_CODE,          1, "u16 raw",        "Active fault code (IEC)",      "—"),
        (REG_ALARM_FLAGS,         1, "u16 raw",        "Alarm bitmask",                "—"),
        // Energy Counters
        (REG_DAILY_ENERGY_KWH,    2, "float32 IE754", "Energy today",                 "kWh"),
        (REG_MONTHLY_ENERGY_KWH,  2, "float32 IE754", "Energy this month",            "kWh"),
        (REG_TOTAL_ENERGY_KWH,    2, "float32 IE754", "Lifetime energy",              "kWh"),
    ];

    let mut info = Vec::new();
    for p in &config.plants {
        let base = p.modbus_mapping.base_address;
        for (offset, regs, dtype, desc, _unit) in LAYOUT {
            info.push(ModbusInfo {
                plant_id:         p.id.clone(),
                register_address: base + offset,
                length:           *regs,
                data_type:        dtype.to_string(),
                description:      format!("{} — {}", desc, p.name),
            });
        }
    }
    Json(info).into_response()
}

// ─── System configuration ─────────────────────────────────────────────────────

/// GET /api/system/config
#[utoipa::path(get, path = "/api/system/config",
    responses((status = 200, description = "Public system configuration", body = SystemConfig)))]
pub async fn get_system_config(State(config): State<Config>) -> impl IntoResponse {
    Json(SystemConfig {
        api_port:            config.server.port,
        modbus_port:         config.modbus.port,
        modbus_host:         "0.0.0.0".to_string(),
        mqtt_enabled:        config.mqtt.enabled,
        mqtt_broker:         if config.mqtt.enabled && !config.mqtt.broker_host.is_empty() {
            Some(format!("{}:{}", config.mqtt.broker_host, config.mqtt.broker_port))
        } else { None },
        mqtt_topic_prefix:   config.mqtt.topic_prefix.clone(),
        websocket_endpoint:  "/ws/telemetry".to_string(),
        prometheus_endpoint: "/metrics".to_string(),
    })
}

// ─── Health check ────────────────────────────────────────────────────────────

/// GET /health
#[utoipa::path(get, path = "/health",
    responses((status = 200, description = "System health", body = HealthStatus)))]
pub async fn health_check(
    State(state): State<AppState>,
    State(config): State<Config>,
) -> impl IntoResponse {
    let all = state.get_all_data();
    let online = all.values().filter(|d| d.status == 1 || d.status == 5).count();
    Json(HealthStatus {
        status:         "ok".to_string(),
        version:        env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: state.uptime_seconds(),
        plants_online:  online,
        plants_total:   config.plants.len(),
        offline_mode:   state.is_offline(),
        mqtt_connected: state.mqtt_connected.load(std::sync::atomic::Ordering::Relaxed),
    })
}

// ─── Prometheus metrics endpoint ─────────────────────────────────────────────

/// GET /metrics  — Prometheus text format
pub async fn prometheus_metrics(State(state): State<AppState>) -> impl IntoResponse {
    let all = state.get_all_data();
    let mut out = String::with_capacity(4096);

    out.push_str("# HELP solar_power_kw Active power output in kW\n");
    out.push_str("# TYPE solar_power_kw gauge\n");
    for (id, d) in &all {
        out.push_str(&format!("solar_power_kw{{plant=\"{}\"}} {:.4}\n", id, d.power_kw));
    }

    out.push_str("# HELP solar_dc_power_kw DC input power in kW\n");
    out.push_str("# TYPE solar_dc_power_kw gauge\n");
    for (id, d) in &all {
        out.push_str(&format!("solar_dc_power_kw{{plant=\"{}\"}} {:.4}\n", id, d.dc_power_kw));
    }

    out.push_str("# HELP solar_efficiency_percent Inverter efficiency %\n");
    out.push_str("# TYPE solar_efficiency_percent gauge\n");
    for (id, d) in &all {
        out.push_str(&format!("solar_efficiency_percent{{plant=\"{}\"}} {:.2}\n", id, d.efficiency_percent));
    }

    out.push_str("# HELP solar_voltage_l1_v Phase L1 voltage in V\n");
    out.push_str("# TYPE solar_voltage_l1_v gauge\n");
    for (id, d) in &all {
        out.push_str(&format!("solar_voltage_l1_v{{plant=\"{}\"}} {:.3}\n", id, d.voltage_l1_v));
    }

    out.push_str("# HELP solar_frequency_hz Grid frequency in Hz\n");
    out.push_str("# TYPE solar_frequency_hz gauge\n");
    for (id, d) in &all {
        out.push_str(&format!("solar_frequency_hz{{plant=\"{}\"}} {:.4}\n", id, d.frequency_hz));
    }

    out.push_str("# HELP solar_temperature_c Cell temperature in °C\n");
    out.push_str("# TYPE solar_temperature_c gauge\n");
    for (id, d) in &all {
        out.push_str(&format!("solar_temperature_c{{plant=\"{}\"}} {:.2}\n", id, d.temperature_c));
    }

    out.push_str("# HELP solar_inverter_temp_c Inverter heatsink temperature in °C\n");
    out.push_str("# TYPE solar_inverter_temp_c gauge\n");
    for (id, d) in &all {
        out.push_str(&format!("solar_inverter_temp_c{{plant=\"{}\"}} {:.2}\n", id, d.inverter_temp_c));
    }

    out.push_str("# HELP solar_daily_energy_kwh Energy produced today in kWh\n");
    out.push_str("# TYPE solar_daily_energy_kwh counter\n");
    for (id, d) in &all {
        out.push_str(&format!("solar_daily_energy_kwh{{plant=\"{}\"}} {:.4}\n", id, d.daily_energy_kwh));
    }

    out.push_str("# HELP solar_total_energy_kwh Lifetime energy produced in kWh\n");
    out.push_str("# TYPE solar_total_energy_kwh counter\n");
    for (id, d) in &all {
        out.push_str(&format!("solar_total_energy_kwh{{plant=\"{}\"}} {:.4}\n", id, d.total_energy_kwh));
    }

    out.push_str("# HELP solar_performance_ratio IEC 61724 Performance Ratio\n");
    out.push_str("# TYPE solar_performance_ratio gauge\n");
    for (id, d) in &all {
        out.push_str(&format!("solar_performance_ratio{{plant=\"{}\"}} {:.4}\n", id, d.performance_ratio));
    }

    out.push_str("# HELP solar_poa_irradiance_w_m2 Plane-of-Array irradiance W/m²\n");
    out.push_str("# TYPE solar_poa_irradiance_w_m2 gauge\n");
    for (id, d) in &all {
        out.push_str(&format!("solar_poa_irradiance_w_m2{{plant=\"{}\"}} {:.2}\n", id, d.poa_irradiance_w_m2));
    }

    out.push_str("# HELP solar_isolation_resistance_mohm Isolation resistance DC-ground MΩ\n");
    out.push_str("# TYPE solar_isolation_resistance_mohm gauge\n");
    for (id, d) in &all {
        out.push_str(&format!("solar_isolation_resistance_mohm{{plant=\"{}\"}} {:.3}\n", id, d.isolation_resistance_mohm));
    }

    out.push_str("# HELP solar_status Inverter status (0=Stop,1=Run,2=Fault,3=Curt,4=Start,5=MPPT)\n");
    out.push_str("# TYPE solar_status gauge\n");
    for (id, d) in &all {
        out.push_str(&format!("solar_status{{plant=\"{}\"}} {}\n", id, d.status));
    }

    out.push_str("# HELP solar_alarm_flags Active alarm bitmask\n");
    out.push_str("# TYPE solar_alarm_flags gauge\n");
    for (id, d) in &all {
        out.push_str(&format!("solar_alarm_flags{{plant=\"{}\"}} {}\n", id, d.alarm_flags));
    }

    out.push_str("# HELP solar_active_alarms_count Number of currently active alarms\n");
    out.push_str("# TYPE solar_active_alarms_count gauge\n");
    for (id, _) in &all {
        let cnt = state.get_active_alarms(Some(id)).len();
        out.push_str(&format!("solar_active_alarms_count{{plant=\"{}\"}} {}\n", id, cnt));
    }

    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        out,
    )
}

// ─── Alarm endpoints ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AlarmQuery {
    pub active_only: Option<bool>,
    pub limit: Option<usize>,
}

/// GET /api/plants/{id}/alarms
#[utoipa::path(get, path = "/api/plants/{id}/alarms",
    params(("id" = String, Path, description = "Plant ID")),
    responses((status = 200, description = "Alarm list", body = Vec<Alarm>)))]
pub async fn get_plant_alarms(
    Path(id): Path<String>,
    Query(q): Query<AlarmQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let alarms = if q.active_only.unwrap_or(false) {
        state.get_active_alarms(Some(&id))
    } else {
        state.get_alarms(Some(&id))
    };
    let limit = q.limit.unwrap_or(100);
    Json(alarms.into_iter().take(limit).collect::<Vec<_>>())
}

/// GET /api/alarms
#[utoipa::path(get, path = "/api/alarms",
    responses((status = 200, description = "All alarms across all plants", body = Vec<Alarm>)))]
pub async fn get_all_alarms(
    Query(q): Query<AlarmQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let alarms = if q.active_only.unwrap_or(false) {
        state.get_active_alarms(None)
    } else {
        state.get_alarms(None)
    };
    let limit = q.limit.unwrap_or(200);
    Json(alarms.into_iter().take(limit).collect::<Vec<_>>())
}

/// DELETE /api/plants/{id}/alarms  — acknowledge all active alarms
pub async fn clear_plant_alarms(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    state.clear_plant_alarms(&id);
    Json(serde_json::json!({"ok": true, "plant_id": id}))
}

// ─── Event log ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct EventQuery {
    pub limit: Option<usize>,
}

/// GET /api/events
#[utoipa::path(get, path = "/api/events",
    responses((status = 200, description = "System event log", body = Vec<Event>)))]
pub async fn get_events(
    Query(q): Query<EventQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(100).min(1000);
    Json(state.get_events(limit))
}

// ─── Settings: Offline Mode ──────────────────────────────────────────────────

/// GET /api/settings/offline-mode
#[utoipa::path(get, path = "/api/settings/offline-mode",
    responses((status = 200, description = "{ offline_mode: bool }")))]
pub async fn get_offline_mode(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({ "offline_mode": state.is_offline() }))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct OfflineModeBody {
    pub enabled: bool,
}

/// POST /api/settings/offline-mode
#[utoipa::path(post, path = "/api/settings/offline-mode",
    responses((status = 200, description = "{ offline_mode: bool, message: string }")))]
pub async fn set_offline_mode(
    State(state): State<AppState>,
    Json(body): Json<OfflineModeBody>,
) -> impl IntoResponse {
    state.set_offline(body.enabled);
    let msg = if body.enabled {
        "Offline mode ENABLED — using solar geometry algorithm"
    } else {
        "Online mode ENABLED — fetching from Open-Meteo API"
    };
    println!("[SETTINGS] {}", msg);
    Json(serde_json::json!({ "offline_mode": body.enabled, "message": msg }))
}

// ─── WebSocket real-time telemetry ────────────────────────────────────────────

/// GET /ws/telemetry — WebSocket endpoint streaming all plant telemetry at 2s
pub async fn ws_telemetry(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut interval = tokio::time::interval(Duration::from_secs(2));

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let all = state.get_all_data();
                let payload = serde_json::json!({
                    "type": "telemetry",
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "plants": all,
                });
                if sender.send(Message::Text(payload.to_string().into())).await.is_err() {
                    break; // client disconnected
                }
            }
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(d))) => {
                        let _ = sender.send(Message::Pong(d)).await;
                    }
                    _ => {}
                }
            }
        }
    }
}


