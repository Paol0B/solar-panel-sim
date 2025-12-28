use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use crate::models::power::PlantData;

#[derive(Clone, Debug)]
pub struct AppState {
    /// Map of plant_id to current plant data
    pub plant_data: Arc<RwLock<HashMap<String, PlantData>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            plant_data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn set_data(&self, plant_id: &str, power: f64, temperature: f64, nominal_power: f64, weather_code: u16, is_day: bool) {
        if let Ok(mut map) = self.plant_data.write() {
            let data = map.entry(plant_id.to_string()).or_default();
            data.power_kw = power;
            data.temperature_c = temperature;
            data.weather_code = weather_code;
            data.is_day = is_day;
            
            // Simulation logic
            
            // Voltage: 230V nominal + rise due to power injection + random noise
            // V = 230 + (P/P_nom * 5) + noise
            // We need nominal power here. Passed as argument.
            let load_factor = if nominal_power > 0.0 { power / nominal_power } else { 0.0 };
            
            // Simple random noise (deterministic for now based on power to avoid needing RNG in state)
            // In a real app we might use rand::thread_rng()
            let noise = (power * 13.0).sin() * 0.5; 
            
            data.voltage_v = 230.0 + (load_factor * 5.0) + noise;
            
            // Frequency: 50Hz + random noise
            let freq_noise = (power * 7.0).cos() * 0.05;
            data.frequency_hz = 50.0 + freq_noise;
            
            data.current_a = if data.voltage_v > 0.0 {
                (power * 1000.0) / data.voltage_v
            } else {
                0.0
            };
            
            data.status = if power > 0.001 { 1 } else { 0 }; // 1=Running, 0=Stopped

            // Efficiency drops with temperature (approx -0.4% per degree above 25C)
            let temp_loss = (temperature - 25.0).max(0.0) * 0.004;
            data.efficiency_percent = (99.0 - (temp_loss * 100.0)).max(90.0);

            // Simple integration for daily energy (assuming 5s interval, this is very rough approximation)
            // In a real system, this would be accumulated properly over time
            // Here we just increment it slightly to show movement
            data.daily_energy_kwh += power * (5.0 / 3600.0); 
        }
    }

    pub fn get_data(&self, plant_id: &str) -> Option<PlantData> {
        if let Ok(map) = self.plant_data.read() {
            map.get(plant_id).cloned()
        } else {
            None
        }
    }
    
    #[allow(dead_code)]
    pub fn get_all_data(&self) -> HashMap<String, PlantData> {
        if let Ok(map) = self.plant_data.read() {
            map.clone()
        } else {
            HashMap::new()
        }
    }
}
