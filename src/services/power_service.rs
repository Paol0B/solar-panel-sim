use chrono::{DateTime, Utc};
use reqwest::Error;

use crate::models::power::{
    CurrentWeatherResponse,
    SimulationData,
};
use crate::services::solar_algorithm;

fn estimate_cell_temperature(ambient_temp_c: f64, g_w_m2: f64) -> f64 {
    // T_cell = T_ambient + (NOCT - 20) * (G / 800)   (NOCT ≈ 45 °C, c-Si typical)
    let noct = 45.0;
    ambient_temp_c + (noct - 20.0) * (g_w_m2 / 800.0)
}

fn estimate_power_kw_from_radiation(g_w_m2: f64, nominal_power_kw: f64, cell_temp_c: f64) -> f64 {
    let alpha = -0.004; // temperature coefficient %/°C
    let temp_factor = 1.0 + alpha * (cell_temp_c - 25.0);
    (nominal_power_kw * (g_w_m2 / 1000.0) * temp_factor).max(0.0)
}

/// Fetch current data from Open-Meteo API; falls back to offline on failure.
pub async fn get_current_data(
    lat: f64,
    lon: f64,
    nominal_power_kw: f64,
) -> Result<SimulationData, Error> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=shortwave_radiation,temperature_2m,weather_code,is_day",
        lat, lon
    );

    match reqwest::get(&url).await {
        Ok(response) => {
            match response.json::<CurrentWeatherResponse>().await {
                Ok(resp) => {
                    let g           = resp.current.shortwave_radiation.unwrap_or(0.0);
                    let ambient_t   = resp.current.temperature_2m.unwrap_or(20.0);
                    let weather_c   = resp.current.weather_code.unwrap_or(0);
                    let is_day      = resp.current.is_day.unwrap_or(1) == 1;
                    let cell_temp   = estimate_cell_temperature(ambient_t, g);
                    let power_kw    = estimate_power_kw_from_radiation(g, nominal_power_kw, cell_temp);

                    let ts_fixed    = format!("{}:00Z", resp.current.time);
                    let timestamp   = ts_fixed.parse::<DateTime<Utc>>().unwrap_or(Utc::now());

                    // Cloud factor approximated from the radiation value
                    let cloud_guessed = if g > 10.0 { (g / 1000.0).min(1.0) } else { 0.0 };

                    return Ok(SimulationData {
                        timestamp,
                        power_kw,
                        temperature_c: cell_temp,
                        ambient_temp_c: ambient_t,
                        weather_code: weather_c,
                        is_day,
                        poa_irradiance_w_m2: g,
                        cloud_factor: cloud_guessed,
                        solar_elevation_deg: 0.0, // not available from Open-Meteo
                    });
                }
                Err(e) => eprintln!("Failed to parse weather data: {}", e),
            }
        }
        Err(e) => eprintln!("Failed to fetch weather data: {}", e),
    }

    // API failed → fall back to offline algorithm
    Ok(get_offline_data(lat, lon, nominal_power_kw))
}

/// Pure offline estimation — no network calls.
pub fn get_offline_data(lat: f64, lon: f64, nominal_power_kw: f64) -> SimulationData {
    let now = Utc::now();
    let est = solar_algorithm::estimate(lat, lon, nominal_power_kw, now);
    SimulationData {
        timestamp: now,
        power_kw:             est.power_kw,
        temperature_c:        est.cell_temp_c,
        ambient_temp_c:       est.ambient_temp_c,
        weather_code:         est.weather_code,
        is_day:               est.is_day,
        poa_irradiance_w_m2:  est.ghi_w_m2,
        cloud_factor:         est.cloud_factor,
        solar_elevation_deg:  est.solar_elevation_deg,
    }
}

