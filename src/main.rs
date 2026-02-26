mod routes;
mod controllers;
mod services;
mod models;
mod api_docs;
mod shared_state;
mod modbus_server;
mod config;

use std::net::SocketAddr;
use std::time::Duration;
use axum::{Router, routing::get, response::Html};
use crate::routes::power_routes::api_routes;
use utoipa::OpenApi;
use utoipa_scalar::Scalar;
use crate::api_docs::ApiDoc;
use crate::shared_state::{AppState, SharedState};
use crate::config::Config;

use std::collections::HashMap;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    // 1. Load configuration
    let config = match Config::load("config.json") {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load config.json: {}", e);
            return;
        }
    };
    println!("Configuration loaded: {} plants", config.plants.len());

    // 2. Initialize shared state (seed offline flag from config)
    let state = AppState::new(config.offline_mode);
    if config.offline_mode {
        println!("[MODE] Offline mode ENABLED — using solar geometry algorithm");
    } else {
        println!("[MODE] Online mode — will fetch from Open-Meteo API");
    }

    // 3. Start background tasks for each plant
    for plant in &config.plants {
        let state_clone = state.clone();
        let plant_config = plant.clone();
        
        tokio::spawn(async move {
            loop {
                let offline = state_clone.is_offline();
                let result = if offline {
                    // Pure offline – no API call
                    let data = services::power_service::get_offline_data(
                        plant_config.latitude,
                        plant_config.longitude,
                        plant_config.nominal_power_kw,
                    );
                    Ok(data)
                } else {
                    // Online: call Open-Meteo, falls back to offline on error
                    services::power_service::get_current_data(
                        plant_config.latitude,
                        plant_config.longitude,
                        plant_config.nominal_power_kw,
                    ).await
                };

                match result {
                    Ok(data) => {
                        let mode_tag = if offline { "OFFLINE" } else { "ONLINE" };
                        state_clone.set_data(
                            &plant_config.id,
                            data.power_kw,
                            data.temperature_c,
                            data.ambient_temp_c,
                            plant_config.nominal_power_kw,
                            data.weather_code,
                            data.is_day,
                            data.poa_irradiance_w_m2,
                            data.cloud_factor,
                            data.solar_elevation_deg,
                        );
                        println!(
                            "[{} UPDATE] Plant: {} | DC Power: {:.2} kW | Temp: {:.1}°C",
                            mode_tag, plant_config.id, data.power_kw, data.temperature_c
                        );
                    }
                    Err(e) => {
                        eprintln!("Error updating plant {}: {}", plant_config.id, e);
                    }
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
    }

    // 4. Start Modbus TCP server
    let modbus_port = config.modbus.port;
    let modbus_addr = SocketAddr::from(([0, 0, 0, 0], modbus_port));
    let state_modbus = state.clone();

    // Build register map: each plant gets a 100-register block starting at base_address.
    // Float32 values → 2 u16 registers (IEEE 754 BE, high word first).
    // u16 values      → 1 register.
    use modbus_server::*;
    let mut register_map = HashMap::new();
    for plant in &config.plants {
        let base = plant.modbus_mapping.base_address;

        macro_rules! ins_f {
            ($off:expr, $vt:ident) => {
                register_map.insert(base + $off,     (plant.id.clone(), VariableType::$vt, 0u8));
                register_map.insert(base + $off + 1, (plant.id.clone(), VariableType::$vt, 1u8));
            };
        }
        macro_rules! ins_u {
            ($off:expr, $vt:ident) => {
                register_map.insert(base + $off, (plant.id.clone(), VariableType::$vt, 0u8));
            };
        }

        // AC Output
        ins_f!(REG_POWER_KW,            PowerKw);
        ins_f!(REG_VOLTAGE_L1_V,        VoltageL1V);
        ins_f!(REG_CURRENT_L1_A,        CurrentL1A);
        ins_f!(REG_FREQUENCY_HZ,        FrequencyHz);
        ins_f!(REG_TEMPERATURE_C,       TemperatureC);
        ins_u!(REG_STATUS,              Status);
        ins_f!(REG_VOLTAGE_L2_V,        VoltageL2V);
        ins_f!(REG_VOLTAGE_L3_V,        VoltageL3V);
        ins_f!(REG_CURRENT_L2_A,        CurrentL2A);
        ins_f!(REG_CURRENT_L3_A,        CurrentL3A);
        ins_f!(REG_REACTIVE_POWER_KVAR, ReactivePowerKvar);
        ins_f!(REG_APPARENT_POWER_KVA,  ApparentPowerKva);
        ins_f!(REG_POWER_FACTOR,        PowerFactor);
        ins_f!(REG_ROCOF_HZ_S,          RocofHzS);
        // DC / MPPT
        ins_f!(REG_DC_VOLTAGE_V,        DcVoltageV);
        ins_f!(REG_DC_CURRENT_A,        DcCurrentA);
        ins_f!(REG_DC_POWER_KW,         DcPowerKw);
        ins_f!(REG_MPPT_VOLTAGE_V,      MpptVoltageV);
        ins_f!(REG_MPPT_CURRENT_A,      MpptCurrentA);
        // Thermal
        ins_f!(REG_INVERTER_TEMP_C,     InverterTempC);
        ins_f!(REG_AMBIENT_TEMP_C,      AmbientTempC);
        // Performance & Irradiance
        ins_f!(REG_EFFICIENCY_PCT,      EfficiencyPct);
        ins_f!(REG_POA_IRRADIANCE,      PoaIrradianceWM2);
        ins_f!(REG_SOLAR_ELEVATION,     SolarElevationDeg);
        ins_f!(REG_PERF_RATIO,          PerformanceRatio);
        ins_f!(REG_SPECIFIC_YIELD,      SpecificYieldKwhKwp);
        ins_f!(REG_CAPACITY_FACTOR,     CapacityFactorPct);
        // Safety & Alarms
        ins_f!(REG_ISOLATION_MOHM,      IsolationMohm);
        ins_u!(REG_FAULT_CODE,          FaultCode);
        ins_u!(REG_ALARM_FLAGS,         AlarmFlags);
        // Energy Counters
        ins_f!(REG_DAILY_ENERGY_KWH,    DailyEnergyKwh);
        ins_f!(REG_MONTHLY_ENERGY_KWH,  MonthlyEnergyKwh);
        ins_f!(REG_TOTAL_ENERGY_KWH,    TotalEnergyKwh);

        println!(
            "[MODBUS] Plant: {} | base={} | regs {}..{} (63 variables, 100-reg block)",
            plant.id, base, base, base + 62
        );
    }

    tokio::spawn(async move {
        if let Err(e) = modbus_server::run_server(modbus_addr, state_modbus, register_map).await {
            eprintln!("Modbus server error: {}", e);
        }
    });

    // 5. Optionally start MQTT publisher
    if config.mqtt.enabled {
        let mqtt_cfg   = config.mqtt.clone();
        let mqtt_state = state.clone();
        let mqtt_plants = config.plants.clone();
        tokio::spawn(async move {
            services::mqtt_service::run_publisher(mqtt_cfg, mqtt_state, mqtt_plants).await;
        });
        println!("[MQTT] Publisher task started → {}:{}", config.mqtt.broker_host, config.mqtt.broker_port);
    }

    // 6. Start Axum HTTP server
    let server_port = config.server.port;
    let shared = SharedState { app: state.clone(), config: config.clone() };

    let app = Router::new()
        // Top-level routes (health, metrics, WebSocket telemetry)
        .route("/health",       get(crate::controllers::power_controller::health_check))
        .route("/metrics",      get(crate::controllers::power_controller::prometheus_metrics))
        .route("/ws/telemetry", get(crate::controllers::power_controller::ws_telemetry))
        .with_state(shared.clone())
        // API routes nested under /api
        .nest("/api", api_routes(shared))
        .route("/scalar", get(|| async {
            Html(Scalar::new(ApiDoc::openapi()).to_html())
        }))
        .fallback_service(ServeDir::new("static"));

    let addr = SocketAddr::from(([0, 0, 0, 0], server_port));
    println!("─────────────────────────────────────────────────────");
    println!(" Solar Panel Simulator | v{}", env!("CARGO_PKG_VERSION"));
    println!("─────────────────────────────────────────────────────");
    println!(" HTTP API:    http://{}/api", addr);
    println!(" Scalar UI:   http://{}/scalar", addr);
    println!(" Health:      http://{}/health", addr);
    println!(" Metrics:     http://{}/metrics", addr);
    println!(" WebSocket:   ws://{}/ws/telemetry", addr);
    println!(" Modbus TCP:  {}", modbus_addr);
    println!("─────────────────────────────────────────────────────");

    axum_server::bind(addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
