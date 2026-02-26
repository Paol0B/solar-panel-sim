/// ============================================================
///  Offline Solar Irradiance & Power Estimation Engine
///
///  Algorithm pipeline:
///   1. Solar geometry  – declination, equation of time, hour angle,
///                        elevation angle, azimuth angle
///   2. Extraterrestrial irradiance – eccentricity-corrected solar constant
///   3. Clear-sky model  – Ineichen / Bird & Hulstrom simplified:
///                         DNI, DHI, GHI on horizontal plane
///   4. Panel tilt / IAM – irradiance on tilted surface (transposition)
///   5. Climatological cloud/haze factor – latitude + season + deterministic
///                         pseudo-random daily variation
///   6. Ambient temperature model – latitude × season × diurnal cycle
///   7. Cell temperature  – Faiman / Ross model
///   8. Power output      – P = P_nom × (G_poa/1000) × η_temp
/// ============================================================

use chrono::{DateTime, Utc, Datelike, Timelike};
use std::f64::consts::PI;

// ─── Physical constants ──────────────────────────────────────
const SC: f64 = 1361.0; // Solar constant W/m²
const DEG: f64 = PI / 180.0;

// ─── Public output ───────────────────────────────────────────
pub struct OfflineEstimate {
    pub power_kw: f64,
    pub ghi_w_m2: f64,
    pub cell_temp_c: f64,
    pub ambient_temp_c: f64,
    pub weather_code: u16,
    pub is_day: bool,
    pub cloud_factor: f64,
    pub solar_elevation_deg: f64,
    /// Wind speed at 10 m (m/s) — affects cell cooling
    pub wind_speed_m_s: f64,
    /// Relative humidity at surface (%) — affects dew/soiling
    pub relative_humidity_pct: f64,
    /// Panel soiling factor [0..1] (1.0 = perfectly clean panel)
    pub soiling_factor: f64,
}

/// Main entry point – call once per update cycle.
///
/// * `lat_deg`  – geographic latitude  (−90 … +90)
/// * `lon_deg`  – geographic longitude (−180 … +180)
/// * `nominal_power_kw` – peak DC capacity of the plant
/// * `utc_now`  – current UTC timestamp (from Utc::now())
pub fn estimate(
    lat_deg: f64,
    lon_deg: f64,
    nominal_power_kw: f64,
    utc_now: DateTime<Utc>,
) -> OfflineEstimate {
    // ── 1. Time decomposition ──────────────────────────────────
    let doy = utc_now.ordinal() as f64; // 1-365/366
    let ut_h = utc_now.hour() as f64
        + utc_now.minute() as f64 / 60.0
        + utc_now.second() as f64 / 3600.0; // UTC decimal hour

    // ── 2. Solar geometry ──────────────────────────────────────
    // a) Declination (Spencer 1971, degrees)
    let b = 2.0 * PI * (doy - 1.0) / 365.0;
    let decl_deg = (180.0 / PI)
        * (0.006918
            - 0.399912 * b.cos()
            + 0.070257 * b.sin()
            - 0.006758 * (2.0 * b).cos()
            + 0.000907 * (2.0 * b).sin()
            - 0.002697 * (3.0 * b).cos()
            + 0.00148 * (3.0 * b).sin());
    let decl = decl_deg * DEG;

    // b) Equation of Time (minutes, Spencer 1971)
    let eot_min = 229.18
        * (0.000075
            + 0.001868 * b.cos()
            - 0.032077 * b.sin()
            - 0.014615 * (2.0 * b).cos()
            - 0.04089 * (2.0 * b).sin());

    // c) Local Solar Time (hours)
    let lstm_deg = 15.0 * (lon_deg / 15.0).round(); // Standard meridian
    let tc_min = 4.0 * (lon_deg - lstm_deg) + eot_min; // Time correction
    // UTC offset from longitude (approximate)
    let utc_offset_h = (lon_deg / 15.0).round();
    let local_clock_h = (ut_h + utc_offset_h).rem_euclid(24.0);
    let lst_h = local_clock_h + tc_min / 60.0; // Local Solar Time

    // d) Hour angle (degrees; negative in morning, positive afternoon)
    let omega_deg = 15.0 * (lst_h - 12.0);
    let omega = omega_deg * DEG;

    // e) Solar elevation angle
    let lat = lat_deg * DEG;
    let sin_alpha = lat.sin() * decl.sin() + lat.cos() * decl.cos() * omega.cos();
    let alpha_rad = sin_alpha.asin(); // elevation (rad)
    let alpha_deg = alpha_rad / DEG;

    // f) Solar azimuth (degrees from North, clockwise)
    let cos_az = if alpha_rad.cos().abs() > 1e-9 {
        (decl.sin() - sin_alpha * lat.sin()) / (alpha_rad.cos() * lat.cos())
    } else {
        0.0
    };
    let az_abs = cos_az.clamp(-1.0, 1.0).acos() / DEG;
    let azimuth_deg = if omega_deg > 0.0 { 360.0 - az_abs } else { az_abs }; // N=0°

    // ── 3. Extraterrestrial irradiance (eccentricity correction) ─
    let e0 = SC * (1.00011
        + 0.034221 * b.cos()
        + 0.00128 * b.sin()
        + 0.000719 * (2.0 * b).cos()
        + 0.000077 * (2.0 * b).sin());

    // ── 4. Clear-sky model (Bird & Hulstrom simplified) ────────
    let (ghi_cs, dni_cs) = if alpha_deg > 0.1 {
        // Air mass – Kasten & Young (1989)
        let am = 1.0
            / (sin_alpha
                + 0.50572 * (alpha_deg + 6.07995_f64).powf(-1.6364));
        let am = am.max(1.0);

        // Transmittance components (simplified Bird & Hulstrom)
        // Rayleigh
        let tr = (-0.0903 * am.powf(0.84) * (1.0 + am - am.powf(1.01))).exp();
        // Ozone (standard column 0.3 atm-cm)
        let to = 1.0 - 0.0013 * am;
        // Aerosol: variable Linke turbidity TL (1.5 = pristine, 6.5 = heavy haze)
        // Continental baseline 3.0; higher in winter (less vertical mixing, more haze)
        let season_turb = if lat_deg >= 0.0 {
            // NH: more turbid in winter (dec-jan) and late summer (sep dust); cleaner in spring
            2.5 + 0.8 * (-(2.0 * PI * (doy - 200.0) / 365.0).cos())
        } else {
            2.5 + 0.8 * ((2.0 * PI * (doy - 20.0) / 365.0).cos())
        };
        // Daily pseudo-random aerosol noise ±0.7 (wind events, fires, dust storms)
        let turb_seed = ((lat_deg * 50.0) as i64).wrapping_mul(503)
            ^ ((lon_deg * 50.0) as i64).wrapping_mul(719)
            ^ (doy as i64).wrapping_mul(1237);
        let turb_noise = ((turb_seed.wrapping_mul(0x517cc1b727220a95_u64 as i64)) >> 11)
            as f64 / (1i64 << 53) as f64;
        let tk = (season_turb + (turb_noise - 0.5) * 1.4).clamp(1.5, 6.5);
        let ta = (-0.09 * tk.powf(0.978) * am.powf(0.9455)).exp();
        // Water vapour (moderate precipitable water 1.5 cm)
        let tw = 1.0 - 0.0075 * am.powf(0.65);

        let total_t = tr * to * ta * tw;
        let dni_cs = 0.9762 * e0 * total_t;
        // Diffuse (sky scatter + back-scatter)
        let dhi_cs = 0.79 * e0 * sin_alpha * (1.0 - total_t)
            * (0.5 * (1.0 - tr) + ba_scatter_coeff(ta))
            / (1.0 - am + am.powf(1.02));
        let ghi_cs = (dni_cs * sin_alpha + dhi_cs).max(0.0);
        (ghi_cs, dni_cs)
    } else {
        (0.0, 0.0)
    };

    // ── 5. Panel tilt / POA irradiance ─────────────────────────
    // Optimal tilt ≈ latitude (fixed-tilt south-facing in NH, north-facing in SH)
    let tilt_deg = lat_deg.abs().min(60.0); // cap at 60°
    let tilt = tilt_deg * DEG;
    // Surface azimuth: 180° (south) NH; 0° (north) SH
    let surf_az_deg = if lat_deg >= 0.0 { 180.0 } else { 0.0 };
    let _surf_az = surf_az_deg * DEG;

    // Angle of incidence (θ) between sun and panel normal
    let az_diff = (azimuth_deg - surf_az_deg) * DEG;
    let cos_theta = if alpha_deg > 0.1 {
        (alpha_rad.sin() * tilt.cos()
            + alpha_rad.cos() * tilt.sin() * az_diff.cos())
        .max(0.0)
    } else {
        0.0
    };

    // Beam irradiance on tilted plane
    let beam_poa = dni_cs * cos_theta;

    // Diffuse (isotropic sky model)
    let dhi_cs = (ghi_cs - dni_cs * sin_alpha.max(0.0)).max(0.0);
    let diffuse_poa = dhi_cs * (1.0 + tilt.cos()) / 2.0;

    // Ground reflected (albedo 0.20)
    let albedo = 0.20;
    let reflected_poa = ghi_cs * albedo * (1.0 - tilt.cos()) / 2.0;

    let ghi_poa_cs = (beam_poa + diffuse_poa + reflected_poa).max(0.0);

    // ── 6. Climatological cloud / haze attenuation ─────────────
    let cloud_factor_base = cloud_attenuation(lat_deg, doy, ut_h, lon_deg);

    // ── 6b. Short-term 5-minute stochastic cloud transient ────
    // Real clouds are broken and intermittent; model a ±18% fluctuation
    // locked to a 5-minute slot (so it's stable within one update cycle).
    let five_min_slot = (ut_h * 12.0) as i64; // 12 slots/hour
    let trans_seed = ((lat_deg * 100.0) as i64).wrapping_mul(853)
        ^ ((lon_deg * 100.0) as i64).wrapping_mul(619)
        ^ (doy as i64 * 300 + five_min_slot).wrapping_mul(1031);
    let trans_val =
        ((trans_seed.wrapping_mul(0x9e3779b97f4a7c15_u64 as i64)) >> 11)
        as f64 / (1i64 << 53) as f64; // [0,1)
    let cloud_transient = (trans_val * 2.0 - 1.0) * 0.18; // ±18%
    let cloud_factor = (cloud_factor_base + cloud_transient).clamp(0.05, 1.0);

    let ghi_poa = ghi_poa_cs * cloud_factor;

    // ── 7. Ambient temperature model ──────────────────────────
    let ambient_temp_c = ambient_temperature(lat_deg, doy, lst_h);

    // ── 7b. Wind speed at 10 m (diurnal + seasonal + daily noise) ─
    let wind_speed = wind_speed_model(lat_deg, lon_deg, doy, lst_h);

    // ── 7c. Relative humidity ──────────────────────────────────
    let relative_humidity = relative_humidity_model(lat_deg, doy, lst_h);

    // ── 8. Cell temperature (Faiman 2008) ─────────────────────
    // T_cell = T_ambient + G_poa * (U0 + U1 * wind)^-1
    // U0=25 W/(m²·K), U1=6.84 W/(m²·K·(m/s)) — crystalline Si
    let u0 = 25.0_f64;
    let u1 = 6.84_f64;
    let cell_temp = ambient_temp_c + ghi_poa / (u0 + u1 * wind_speed);

    // ── 8b. Panel soiling factor ───────────────────────────────
    // Dust accumulates at 0.3%/day; rain (cloudy days) clears it.
    let soiling_factor = panel_soiling_factor(lat_deg, lon_deg, doy);

    // ── 9. DC Power: temperature + soiling coefficients ────────
    let alpha_temp = -0.004; // %/°C for typical c-Si
    let temp_factor = 1.0 + alpha_temp * (cell_temp - 25.0);
    // Apply soiling as an effective irradiance reduction
    let effective_ghi = ghi_poa * soiling_factor;
    let power_kw = (nominal_power_kw * (effective_ghi / 1000.0) * temp_factor).max(0.0);

    // ── 10. Synthetic weather code (WMO-like)  ─────────────────
    let weather_code = synthetic_weather_code(cloud_factor, alpha_deg, doy, lat_deg);

    let is_day = alpha_deg > 0.0 && ghi_poa > 0.5;

    OfflineEstimate {
        power_kw,
        ghi_w_m2: ghi_poa,
        cell_temp_c: cell_temp,
        ambient_temp_c,
        weather_code,
        is_day,
        cloud_factor,
        solar_elevation_deg: alpha_deg,
        wind_speed_m_s: wind_speed,
        relative_humidity_pct: relative_humidity,
        soiling_factor,
    }
}

// ─── Helper: back-scatter term for Bird diffuse ──────────────
#[inline]
fn ba_scatter_coeff(ta: f64) -> f64 {
    // Approximated from Bird (1981) Table 2
    0.5 * (0.92 - ta.ln().abs() / 10.0).max(0.2).min(0.5)
}

// ─── Climatological cloud attenuation ────────────────────────
/// Returns a factor in [0, 1] representing the fraction of clear-sky GHI
/// that actually reaches the panel on average for the given location & season.
///
/// The model layers three effects:
///  a) Climate-zone baseline cloudiness (based on latitude band / season)
///  b) Slow day-to-day variation (sinusoidal, seeded from plant location + DOY)
///  c) Intra-day variation (morning / afternoon cloud build-up typical of
///     continental climates)
fn cloud_attenuation(lat_deg: f64, doy: f64, lst_h: f64, lon_deg: f64) -> f64 {
    // --- a) Baseline clearness index by latitude/season ---
    // Northern hemisphere: summer clear (high), winter less clear
    // Southern hemisphere: inverted phase
    // Equatorial band: persistent 40-60 % cloud cover

    let season_phase = if lat_deg >= 0.0 {
        // NH: max clearness ~day 180 (mid-June), min ~day 355 (late Dec)
        (2.0 * PI * (doy - 180.0) / 365.0).cos()
    } else {
        // SH: inverted
        (2.0 * PI * (doy - 365.0) / 365.0).cos()
    };

    // Latitude effect: equatorial ~0.55, mid-lat ~0.65, polar ~0.50
    let lat_factor = {
        let abs_lat = lat_deg.abs();
        if abs_lat < 15.0 {
            // Tropical band: consistently cloudy/humid
            0.55 + 0.05 * season_phase
        } else if abs_lat < 35.0 {
            // Subtropical (desert belt in NH: Mediterranean, Sahara zone)
            0.70 + 0.10 * season_phase
        } else if abs_lat < 55.0 {
            // Mid-latitude temperate
            0.62 + 0.12 * season_phase
        } else if abs_lat < 65.0 {
            // Sub-polar
            0.52 + 0.10 * season_phase
        } else {
            // Polar
            0.45 + 0.10 * season_phase
        }
    };

    // --- b) Day-to-day pseudo-random variation ----------------
    // Deterministic hash: changes every day, consistent for same plant × day
    let seed = ((lat_deg * 100.0) as i64).wrapping_mul(397)
        ^ ((lon_deg * 100.0) as i64).wrapping_mul(631)
        ^ (doy as i64).wrapping_mul(1013);
    // Map seed to [-1, 1] smoothly
    let daily_noise = ((seed % 1000) as f64 / 1000.0 - 0.5) * 2.0; // [-1,1]
    let day_variation = daily_noise * 0.12; // ±12% daily scatter

    // --- c) Intra-day variation --------------------------------
    // Clouds tend to build up in afternoon in continental areas
    // Apply small cosine curve centered on 10:00 solar (less cloud in AM)
    let intraday = if lst_h >= 6.0 && lst_h <= 20.0 {
        let x = (lst_h - 13.0) / 7.0; // -1 at 06:00, +1 at 20:00
        -0.05 * x // slight penalty in afternoon
    } else {
        0.0
    };

    (lat_factor + day_variation + intraday).clamp(0.15, 1.0)
}

// ─── Ambient temperature model ───────────────────────────────
/// Estimates ambient 2 m temperature (°C) from:
///  - latitude × season (mean annual temperature + amplitude)
///  - diurnal cycle (min ~6 h before solar noon, max ~2 h after solar noon)
fn ambient_temperature(lat_deg: f64, doy: f64, lst_h: f64) -> f64 {
    let abs_lat = lat_deg.abs();

    // Mean annual temperature by latitude (rough model)
    let t_annual_mean = if abs_lat < 10.0 {
        27.0
    } else if abs_lat < 25.0 {
        22.0
    } else if abs_lat < 40.0 {
        15.0
    } else if abs_lat < 55.0 {
        8.0
    } else if abs_lat < 66.5 {
        1.0
    } else {
        -10.0
    };

    // Annual amplitude (larger at mid/high latitudes)
    let t_amplitude = if abs_lat < 10.0 {
        2.0
    } else if abs_lat < 25.0 {
        7.0
    } else if abs_lat < 40.0 {
        12.0
    } else if abs_lat < 55.0 {
        14.0
    } else {
        12.0
    };

    // Seasonal variation (NH: warmest ~day 200; SH: reversed)
    let season_angle = if lat_deg >= 0.0 {
        2.0 * PI * (doy - 200.0) / 365.0
    } else {
        2.0 * PI * (doy - 20.0) / 365.0
    };
    let t_seasonal = t_annual_mean + t_amplitude * season_angle.cos();

    // Diurnal range ±5°C peak-to-peak on surface
    // Min ~06:00 solar, max ~14:00 solar
    let diurnal_phase = 2.0 * PI * (lst_h - 14.0) / 24.0; // max at 14:00
    let t_diurnal = 5.0 * diurnal_phase.cos();

    t_seasonal + t_diurnal
}

// ─── Synthetic WMO weather code ──────────────────────────────
/// Derives a WMO-like weather code from the computed atmospheric state,
/// so the frontend can render an appropriate weather icon.
fn synthetic_weather_code(cloud_factor: f64, alpha_deg: f64, doy: f64, lat_deg: f64) -> u16 {
    // WMO codes used by open-meteo:
    //  0 = clear sky
    //  1 = mainly clear, 2 = partly cloudy, 3 = overcast
    //  45/48 = fog
    //  51-67 = drizzle / rain
    //  71-77 = snow
    //  95 = thunderstorm
    if alpha_deg <= 0.0 {
        return 0; // night – clear sky code
    }

    // Estimate snowfall risk: high-lat winter
    let abs_lat = lat_deg.abs();
    let winter_day = if lat_deg >= 0.0 {
        doy < 60.0 || doy > 330.0
    } else {
        doy > 150.0 && doy < 270.0
    };
    let snow_likely = abs_lat > 40.0 && winter_day;

    if cloud_factor > 0.85 {
        0 // clear sky
    } else if cloud_factor > 0.75 {
        1 // mainly clear
    } else if cloud_factor > 0.60 {
        2 // partly cloudy
    } else if cloud_factor > 0.45 {
        3 // overcast
    } else if cloud_factor > 0.35 {
        if snow_likely { 71 } else { 61 } // moderate rain / snow
    } else if cloud_factor > 0.25 {
        if snow_likely { 73 } else { 63 }
    } else {
        if snow_likely { 75 } else { 65 } // heavy rain / snow
    }
}

// ─── Wind speed model ────────────────────────────────────────
/// Estimates near-surface wind speed (m/s) at 10 m — affects cell temperature.
///
/// Diurnal pattern: calm at night/dawn, peaks ~14:00 solar (convective mixing).
/// Seasonal: stronger in winter at mid/high latitudes.
/// Daily pseudo-random noise to simulate synoptic variability.
fn wind_speed_model(lat_deg: f64, lon_deg: f64, doy: f64, lst_h: f64) -> f64 {
    let abs_lat = lat_deg.abs();

    // Climatological mean wind speed by latitude band
    let base = if abs_lat < 10.0 { 2.2 }
        else if abs_lat < 25.0   { 3.0 }
        else if abs_lat < 40.0   { 3.8 }
        else if abs_lat < 55.0   { 4.5 }
        else                     { 5.5 };

    // Diurnal cycle: min at ~06:00, max at ~14:00 solar
    let diurnal_amp = base * 0.35;
    let diurnal = diurnal_amp * (2.0 * PI * (lst_h - 14.0) / 24.0).cos().abs();

    // Seasonal: stronger in winter (less solar heating → stronger pressure gradients)
    let season_amp = base * 0.25;
    let season = if lat_deg >= 0.0 {
        // NH: max ~Jan (doy 15), min ~Jul (doy 196)
        season_amp * (-(2.0 * PI * (doy - 200.0) / 365.0).cos())
    } else {
        // SH inverse
        season_amp * ((2.0 * PI * (doy - 20.0) / 365.0).cos())
    };

    // Daily pseudo-random synoptic factor (0.6 – 1.4 × mean)
    let seed = ((lat_deg * 73.0) as i64).wrapping_mul(701)
        ^ ((lon_deg * 73.0) as i64).wrapping_mul(449)
        ^ (doy as i64).wrapping_mul(983);
    let daily_factor = 0.60
        + 0.80 * (((seed.wrapping_mul(0x6c62272e07bb0142_u64 as i64)) >> 11)
            as f64 / (1i64 << 53) as f64);

    // Nighttime calming (0–05:00 and 21–24:00 solar)
    let night_damp = if lst_h < 5.5 || lst_h > 21.5 { 0.45 } else { 1.0 };

    ((base + diurnal + season) * daily_factor * night_damp).clamp(0.3, 18.0)
}

// ─── Relative humidity model ─────────────────────────────────
/// Estimates surface relative humidity (%) based on latitude/season/hour.
///
/// RH is highest at dawn, lowest in early afternoon.
/// Higher in tropical/coastal zones, lower in deserts.
fn relative_humidity_model(lat_deg: f64, doy: f64, lst_h: f64) -> f64 {
    let abs_lat = lat_deg.abs();

    // Climatological mean RH by latitude
    let base = if abs_lat < 10.0 { 78.0 }       // humid tropics
        else if abs_lat < 25.0   { 58.0 }        // subtropics / deserts
        else if abs_lat < 40.0   { 62.0 }        // Mediterranean / mid-lat
        else if abs_lat < 55.0   { 70.0 }        // temperate oceanic
        else                     { 72.0 };       // sub-polar

    // Diurnal: max ~05:00 solar, min ~14:00 (anti-correlated with temperature)
    // Typical diurnal amplitude: ±12-18 RH points
    let diurnal_amp = 14.0;
    let diurnal = diurnal_amp * (2.0 * PI * (lst_h - 5.0) / 24.0).cos();

    // Seasonal (NH: slightly more humid in summer from evaporation)
    let season_amp = 8.0;
    let seasonal = if lat_deg >= 0.0 {
        season_amp * (2.0 * PI * (doy - 200.0) / 365.0).cos()
    } else {
        season_amp * (2.0 * PI * (doy - 20.0) / 365.0).cos()
    };

    (base + diurnal + seasonal).clamp(15.0, 98.0)
}

// ─── Panel soiling model (deterministic accumulation) ────────
/// Returns soiling factor in [0.85, 1.0] (1 = clean).
///
/// Algorithm: walks back up to 30 days to find the most recent rainy day
/// (cloud_factor < 0.40 at noon → rain). Soiling accumulates at ~0.3 %/day.
/// Maximum soiling is −15 % irradiance (30-day dry spell).
fn panel_soiling_factor(lat_deg: f64, lon_deg: f64, doy: f64) -> f64 {
    const SOIL_RATE: f64    = 0.003;   // 0.3 %/day
    const MAX_DAYS: usize   = 30;
    const RAIN_CF: f64      = 0.42;    // cloud_factor below this → rain

    let abs_lat = lat_deg.abs();
    let mut dry_days = 0usize;

    for back in 1..=MAX_DAYS {
        let past_doy = ((doy as i32 - back as i32).rem_euclid(365) + 1) as f64;

        // Reconstruct approximate daily cloud_factor at noon for that past day
        let season_phase = if lat_deg >= 0.0 {
            (2.0 * PI * (past_doy - 180.0) / 365.0).cos()
        } else {
            (2.0 * PI * (past_doy - 365.0) / 365.0).cos()
        };
        let lat_cf_base = if abs_lat < 15.0 { 0.55 + 0.05 * season_phase }
            else if abs_lat < 35.0          { 0.70 + 0.10 * season_phase }
            else if abs_lat < 55.0          { 0.62 + 0.12 * season_phase }
            else if abs_lat < 65.0          { 0.52 + 0.10 * season_phase }
            else                            { 0.45 + 0.10 * season_phase };
        let seed = ((lat_deg * 100.0) as i64).wrapping_mul(397)
            ^ ((lon_deg * 100.0) as i64).wrapping_mul(631)
            ^ (past_doy as i64).wrapping_mul(1013);
        let noise = ((seed % 1000) as f64 / 1000.0 - 0.5) * 2.0;
        let past_cf = (lat_cf_base + noise * 0.12).clamp(0.15, 1.0);

        if past_cf < RAIN_CF {
            // Rained that day → panels washed clean after it
            break;
        }
        dry_days += 1;
    }

    (1.0 - SOIL_RATE * dry_days as f64).clamp(0.85, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_summer_noon_italy() {
        // Turin, Italy – summer solstice noon UTC+2 → 11:00 UTC
        let t = Utc.with_ymd_and_hms(2025, 6, 21, 9, 0, 0).unwrap();
        let r = estimate(45.07, 7.33, 1000.0, t);
        // Should produce meaningful power at summer noon
        assert!(r.solar_elevation_deg > 60.0, "Elevation should be >60° at summer noon, got {:.1}", r.solar_elevation_deg);
        assert!(r.ghi_w_m2 > 400.0, "GHI should be significant, got {:.1}", r.ghi_w_m2);
        assert!(r.power_kw > 200.0, "Power should be significant, got {:.1}", r.power_kw);
        println!("Summer noon Turin: elev={:.1}° GHI={:.0} W/m² power={:.1} kW temp={:.1}°C cloud={:.2}",
            r.solar_elevation_deg, r.ghi_w_m2, r.power_kw, r.cell_temp_c, r.cloud_factor);
    }

    #[test]
    fn test_midnight_zero() {
        // Power at midnight should be 0
        let t = Utc.with_ymd_and_hms(2025, 6, 21, 22, 0, 0).unwrap();
        let r = estimate(45.07, 7.33, 1000.0, t);
        assert_eq!(r.power_kw, 0.0, "Power at night must be 0");
    }

    #[test]
    fn test_winter_solstice() {
        // Turin, winter solstice at solar noon (~UTC 11:40)
        let t = Utc.with_ymd_and_hms(2025, 12, 21, 11, 0, 0).unwrap();
        let r = estimate(45.07, 7.33, 1000.0, t);
        // Winter noon elevation should be much lower than summer
        assert!(r.solar_elevation_deg > 15.0 && r.solar_elevation_deg < 35.0,
            "Winter elevation should be 15-35°, got {:.1}", r.solar_elevation_deg);
        println!("Winter noon Turin: elev={:.1}° GHI={:.0} W/m² power={:.1} kW",
            r.solar_elevation_deg, r.ghi_w_m2, r.power_kw);
    }
}
