use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicBool, Ordering};
use crate::models::power::PlantData;

#[derive(Clone, Debug)]
pub struct AppState {
    /// Map of plant_id to current plant data
    pub plant_data: Arc<RwLock<HashMap<String, PlantData>>>,
    /// Offline mode flag — toggled at runtime via API
    pub offline_mode: Arc<AtomicBool>,
}

impl AppState {
    pub fn new(offline_mode_default: bool) -> Self {
        Self {
            plant_data: Arc::new(RwLock::new(HashMap::new())),
            offline_mode: Arc::new(AtomicBool::new(offline_mode_default)),
        }
    }

    pub fn is_offline(&self) -> bool {
        self.offline_mode.load(Ordering::Relaxed)
    }

    pub fn set_offline(&self, value: bool) {
        self.offline_mode.store(value, Ordering::Relaxed);
    }

    pub fn set_data(&self, plant_id: &str, power: f64, temperature: f64, nominal_power: f64, weather_code: u16, is_day: bool) {
        if let Ok(mut map) = self.plant_data.write() {
            let data = map.entry(plant_id.to_string()).or_default();
            
            data.temperature_c = temperature;
            data.weather_code = weather_code;
            data.is_day = is_day;
            
            // Efficiency Calculation
            let input_load_factor = if nominal_power > 0.0 { power / nominal_power } else { 0.0 };
            
            let eff_load_factor = if input_load_factor < 0.01 {
                0.0
            } else if input_load_factor < 0.1 {
                0.80 + (input_load_factor / 0.1) * 0.15 
            } else if input_load_factor < 0.5 {
                0.95 + ((input_load_factor - 0.1) / 0.4) * 0.03 
            } else {
                0.98 - ((input_load_factor - 0.5) / 0.5) * 0.01 
            };

            let temp_loss = (temperature - 25.0).max(0.0) * 0.0005; 
            let efficiency = (eff_load_factor - temp_loss).max(0.0);
            data.efficiency_percent = efficiency * 100.0;

            // AC Power Output
            let ac_power = power * efficiency;
            
            // Voltage (sempre stabile intorno a 230V per inverter grid-tied)
            let ac_load_factor = if nominal_power > 0.0 { ac_power / nominal_power } else { 0.0 };
            let noise = if ac_power > 0.01 { (ac_power * 13.0).sin() * 0.5 } else { 0.0 };
            data.voltage_v = 230.0 + (ac_load_factor * 5.0) + noise;
            
            // Frequency
            let freq_noise = if ac_power > 0.01 { (ac_power * 7.0).cos() * 0.05 } else { 0.0 };
            data.frequency_hz = 50.0 + freq_noise;
            
            // Power Factor (solo se c'è potenza significativa)
            if ac_power > 0.01 {
                let pf_base = 0.95 + 0.05 * (1.0 - (-10.0 * ac_load_factor).exp());
                let pf_noise = (ac_power * 11.0).sin() * 0.005;
                data.power_factor = (pf_base + pf_noise).min(1.0).max(0.8);
            } else {
                data.power_factor = 1.0; // Unity power factor quando non c'è potenza
            }

            // Active Power (settato DOPO il power factor)
            data.power_kw = ac_power;

            // Apparent Power
            if ac_power > 0.01 && data.power_factor > 0.0 {
                data.apparent_power_kva = ac_power / data.power_factor;
            } else {
                data.apparent_power_kva = ac_power;
            }

            // Reactive Power
            let q_sq = data.apparent_power_kva.powi(2) - ac_power.powi(2);
            data.reactive_power_kvar = if q_sq > 0.0 { q_sq.sqrt() } else { 0.0 };

            // Current - sempre positiva
            if ac_power > 0.01 && data.voltage_v > 0.0 {
                data.current_a = (data.apparent_power_kva * 1000.0) / data.voltage_v;
            } else {
                data.current_a = 0.0;
            }
            
            data.status = if ac_power > 0.001 { 1 } else { 0 }; 

            // Energy
            data.daily_energy_kwh += ac_power * (5.0 / 3600.0);
            
            // Debug log for verification
            println!("[STATE UPDATE] Plant: {} | AC Power: {:.2} kW | Current: {:.2} A | Voltage: {:.1} V | PF: {:.3}", 
                     plant_id, data.power_kw, data.current_a, data.voltage_v, data.power_factor);
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
