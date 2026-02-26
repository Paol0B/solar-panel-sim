use axum::{routing::get, Router};
use crate::controllers::power_controller::{
    // Plants & telemetry
    list_plants, get_plant_power, get_global_power,
    // Modbus & config
    get_modbus_info, get_system_config,
    // Alarms & events
    get_plant_alarms, get_all_alarms, clear_plant_alarms, get_events,
    // Settings
    get_offline_mode, set_offline_mode,
};
use crate::shared_state::SharedState;

/// Build the `/api/*` sub-router.
/// Handlers extract `State<AppState>` and/or `State<Config>` via
/// `FromRef<SharedState>` — a single `.with_state(shared)` covers both.
pub fn api_routes(shared: SharedState) -> Router {
    Router::new()
        .route("/plants",                  get(list_plants))
        .route("/plants/{id}/power",       get(get_plant_power))
        .route("/power/global",            get(get_global_power))
        .route("/modbus/info",             get(get_modbus_info))
        .route("/system/config",           get(get_system_config))
        .route("/plants/{id}/alarms",      get(get_plant_alarms).delete(clear_plant_alarms))
        .route("/alarms",                  get(get_all_alarms))
        .route("/events",                  get(get_events))
        .route("/settings/offline-mode",   get(get_offline_mode).post(set_offline_mode))
        .with_state(shared)
}
