use utoipa::OpenApi;
use crate::controllers::power_controller;
use crate::models::power;
use crate::config;

#[derive(OpenApi)]
#[openapi(
    paths(
        power_controller::list_plants,
        power_controller::get_plant_power,
        power_controller::get_global_power,
        power_controller::get_modbus_info,
        power_controller::get_offline_mode,
        power_controller::set_offline_mode
    ),
    components(
        schemas(
            power::PlantData,
            config::PlantConfig,
            power::ModbusInfo
        )
    ),
    tags(
        (name = "solar-panel-sim", description = "Solar Panel Simulation API")
    )
)]
pub struct ApiDoc;
