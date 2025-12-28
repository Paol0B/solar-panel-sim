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
                        // println!("Updated plant {}: {} kW", plant_config.id, data.power_kw);
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
    
    // Create register map from config
    let mut register_map = HashMap::new();
    for plant in &config.plants {
        register_map.insert(plant.modbus_mapping.power_address, (plant.id.clone(), modbus_server::VariableType::Power));
        register_map.insert(plant.modbus_mapping.voltage_address, (plant.id.clone(), modbus_server::VariableType::Voltage));
        register_map.insert(plant.modbus_mapping.current_address, (plant.id.clone(), modbus_server::VariableType::Current));
        register_map.insert(plant.modbus_mapping.frequency_address, (plant.id.clone(), modbus_server::VariableType::Frequency));
        register_map.insert(plant.modbus_mapping.temperature_address, (plant.id.clone(), modbus_server::VariableType::Temperature));
        register_map.insert(plant.modbus_mapping.status_address, (plant.id.clone(), modbus_server::VariableType::Status));
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
