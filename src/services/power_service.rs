use chrono::{DateTime, Utc, Timelike};
use reqwest::Error;

use crate::models::power::{
    CurrentWeatherResponse,
    SimulationData,
};

fn estimate_power_kw_from_radiation(g_w_m2: f64, nominal_power_kw: f64, cell_temp_c: f64) -> f64 {
    // P = P_nom * (G / 1000) * [1 + alpha * (T_cell - 25)]
    // alpha approx -0.004
    let alpha = -0.004;
    let temp_factor = 1.0 + alpha * (cell_temp_c - 25.0);
    
    let raw_power = nominal_power_kw * (g_w_m2 / 1000.0);
    (raw_power * temp_factor).max(0.0)
}

fn estimate_cell_temperature(ambient_temp_c: f64, g_w_m2: f64) -> f64 {
    // Simple model: T_cell = T_ambient + (NOCT - 20) * (G / 800)
    // Assuming NOCT (Nominal Operating Cell Temperature) is around 45°C
    let noct = 45.0;
    ambient_temp_c + (noct - 20.0) * (g_w_m2 / 800.0)
}

/// Current power from Open-Meteo
pub async fn get_current_data(
    lat: f64,
    lon: f64,
    nominal_power_kw: f64,
) -> Result<SimulationData, Error> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=shortwave_radiation,temperature_2m,weather_code,is_day",
        lat, lon
    );

    // Try to fetch from API
    match reqwest::get(&url).await {
        Ok(response) => {
            match response.json::<CurrentWeatherResponse>().await {
                Ok(resp) => {
                    let g = resp.current.shortwave_radiation.unwrap_or(0.0);
                    let ambient_temp = resp.current.temperature_2m.unwrap_or(20.0);
                    let weather_code = resp.current.weather_code.unwrap_or(0);
                    let is_day = resp.current.is_day.unwrap_or(1) == 1;
                    
                    let temperature_c = estimate_cell_temperature(ambient_temp, g);
                    let power_kw = estimate_power_kw_from_radiation(g, nominal_power_kw, temperature_c);

                    // Open-Meteo: "2025-12-28T10:40" -> add ":00Z"
                    let ts_fixed = format!("{}:00Z", resp.current.time);
                    let timestamp = ts_fixed.parse::<DateTime<Utc>>().unwrap_or(Utc::now());

                    return Ok(SimulationData {
                        timestamp,
                        power_kw,
                        temperature_c,
                        weather_code,
                        is_day,
                    });
                }
                Err(e) => eprintln!("Failed to parse weather data: {}", e),
            }
        }
        Err(e) => eprintln!("Failed to fetch weather data: {}", e),
    }

    // Fallback Simulation (Offline Mode)
    // Simulate day/night cycle based on UTC time
    let now = Utc::now();
    let hour = now.hour() as f64 + (now.minute() as f64 / 60.0);
    
    // Simple sun curve between 6:00 and 18:00
    let g_sim = if hour > 6.0 && hour < 18.0 {
        let peak = 1000.0; // W/m2
        // Parabolic curve
        let x = (hour - 12.0) / 6.0; // -1 to 1
        peak * (1.0 - x * x).max(0.0)
    } else {
        0.0
    };

    let ambient_sim = 15.0 + (g_sim / 100.0); // 15C night, up to 25C day
    let temperature_c = estimate_cell_temperature(ambient_sim, g_sim);
    let power_kw = estimate_power_kw_from_radiation(g_sim, nominal_power_kw, temperature_c);
    
    let is_day = g_sim > 0.0;
    let weather_code = if is_day { 0 } else { 0 }; // Clear sky default

    Ok(SimulationData {
        timestamp: now,
        power_kw,
        temperature_c,
        weather_code,
        is_day,
    })
}
