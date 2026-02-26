use axum::{routing::get, Router};
use crate::controllers::power_controller::{
    list_plants, get_plant_power, get_global_power, get_modbus_info,
    get_offline_mode, set_offline_mode,
};
use crate::config::Config;
use crate::shared_state::AppState;

pub fn power_routes(config: Config, state: AppState) -> Router {
    Router::new()
        .route("/plants", get(list_plants))
        .route("/modbus/info", get(get_modbus_info))
        .with_state(config)
        .route("/plants/{id}/power", get(get_plant_power))
        .route("/power/global", get(get_global_power))
        .route("/settings/offline-mode", get(get_offline_mode).post(set_offline_mode))
        .with_state(state)
}
