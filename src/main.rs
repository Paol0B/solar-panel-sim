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
use crate::routes::power_routes::power_routes;
use utoipa::OpenApi;
use utoipa_scalar::Scalar;
use crate::api_docs::ApiDoc;
use crate::shared_state::AppState;
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

    // 2. Initialize shared state
    let state = AppState::new();

    // 3. Start background tasks for each plant
    for plant in &config.plants {
        let state_clone = state.clone();
        let plant_config = plant.clone();
        
        tokio::spawn(async move {
            loop {
                match services::power_service::get_current_data(
                    plant_config.latitude, 
                    plant_config.longitude,
                    plant_config.nominal_power_kw
                ).await {
                    Ok(data) => {
                        state_clone.set_data(
                            &plant_config.id, 
                            data.power_kw, 
                            data.temperature_c,
                            plant_config.nominal_power_kw,
                            data.weather_code,
                            data.is_day
                        );
                        println!("[UPDATE] Plant: {} | DC Power: {:.2} kW | Temp: {:.1}°C", 
                                 plant_config.id, data.power_kw, data.temperature_c);
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
    
    // Create register map from config.
    // Numeric variables (Power, Voltage, Current, Frequency, Temperature) are encoded as
    // IEEE 754 float32 stored in TWO consecutive u16 registers (big-endian: high word at
    // the configured address, low word at configured address + 1).
    // Status occupies a single u16 register (word_index = 0).
    let mut register_map = HashMap::new();
    for plant in &config.plants {
        let m = &plant.modbus_mapping;
        println!(
            "[MODBUS MAP] Plant: {} → Power@{}+{}, Voltage@{}+{}, Current@{}+{}, Freq@{}+{}, Temp@{}+{}, Status@{}",
            plant.id,
            m.power_address,       m.power_address + 1,
            m.voltage_address,     m.voltage_address + 1,
            m.current_address,     m.current_address + 1,
            m.frequency_address,   m.frequency_address + 1,
            m.temperature_address, m.temperature_address + 1,
            m.status_address
        );

        // Helper macro: insert high (word 0) + low (word 1) for a float32 variable.
        macro_rules! ins_float {
            ($addr:expr, $vt:ident) => {
                register_map.insert($addr,     (plant.id.clone(), modbus_server::VariableType::$vt, 0u8));
                register_map.insert($addr + 1, (plant.id.clone(), modbus_server::VariableType::$vt, 1u8));
            };
        }

        ins_float!(m.power_address,       Power);
        ins_float!(m.voltage_address,     Voltage);
        ins_float!(m.current_address,     Current);
        ins_float!(m.frequency_address,   Frequency);
        ins_float!(m.temperature_address, Temperature);
        // Status: single u16, word_index = 0
        register_map.insert(m.status_address, (plant.id.clone(), modbus_server::VariableType::Status, 0u8));
    }
    
    tokio::spawn(async move {
        if let Err(e) = modbus_server::run_server(modbus_addr, state_modbus, register_map).await {
            eprintln!("Modbus server error: {}", e);
        }
    });

    // 5. Start Axum HTTP server
    let server_port = config.server.port;
    let app = Router::new()
        .nest("/api", power_routes(config.clone(), state.clone()))
        .route("/scalar", get(|| async {
            Html(Scalar::new(ApiDoc::openapi()).to_html())
        }))
        .fallback_service(ServeDir::new("static"));

    let addr = SocketAddr::from(([0, 0, 0, 0], server_port));
    println!("API Server listening on http://{}", addr);
    println!("Scalar UI: http://{}/scalar", addr);
    println!("Modbus TCP: {}", modbus_addr);

    axum_server::bind(addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
