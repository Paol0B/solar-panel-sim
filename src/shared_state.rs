use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::Datelike;

use crate::models::power::{
    Alarm, AlarmSeverity, Event, EventKind, PlantData,
    alarm_codes, alarm_flag_bits,
};

const MAX_ALARM_HISTORY: usize  = 500;
const MAX_EVENT_LOG: usize      = 1000;
/// Update interval in seconds (must match main.rs sleep)
const UPDATE_INTERVAL_S: f64   = 5.0;

// ─── Nominal DC string constants (typical c-Si array) ───────────────────────
/// Nominal DC link voltage at STC (V). Real inverters operate 400–800 V DC.
const V_DC_NOM: f64    = 700.0;
/// V_mpp / V_oc ratio (≈0.80 for c-Si)
const VMPP_VOC_RATIO: f64 = 0.80;
/// Temperature coefficient of V_mpp (V/V/°C)
const V_TEMP_COEFF: f64  = -0.0035;

// ─── MPPT startup / shutdown thresholds ─────────────────────────────────────
/// Minimum POA irradiance (W/m²) for the inverter to attempt grid connection
const IRRAD_START_W_M2: f64 = 30.0;
/// Minimum POA irradiance (W/m²) to stay connected (hysteresis below start)
const IRRAD_STOP_W_M2:  f64 = 15.0;
/// Ramp rate per 5-second sample during startup / shutdown (fraction / sample)
const RAMP_RATE: f64 = 0.08; // 0 → 1 in ~12.5 samples ≈ 62 s

// ─── Grid limits (configurable in a real inverter) ──────────────────────────
const V_GRID_NOM: f64       = 230.0;   // V (L-N)
const V_OV_LIMIT: f64       = 253.0;   // +10 % EN 50160
const V_UV_LIMIT: f64       = 207.0;   // -10 %
const F_NOM: f64            = 50.0;    // Hz
const F_OV_LIMIT: f64       = 50.5;    // Hz
const F_UV_LIMIT: f64       = 49.5;    // Hz
const ROCOF_LIMIT: f64      = 1.0;     // Hz/s (VDE 4110)
const ISOL_FAULT_MOHM: f64  = 0.5;    // MΩ — below this triggers isolation fault
const T_OVERTEMP_C: f64     = 80.0;   // °C inverter heatsink trip

// ─── Fault injection probabilities ──────────────────────────────────────────
/// Probability per 5-minute epoch that a grid-voltage swell/sag event fires.
const P_VOLT_FAULT: f64    = 0.025;  // ~1 event / 83 min per plant
/// Probability per 5-minute epoch for an over/under-frequency event.
const P_FREQ_FAULT: f64    = 0.015;  // ~1 event / ~2.8 h per plant
/// Probability per 1-hour epoch for an isolation-resistance fault.
const P_ISOL_FAULT: f64    = 0.015;  // ~1 event / 67 h per plant (heavy rain)
/// Probability per 15-minute epoch for an overtemperature event.
const P_OT_FAULT: f64      = 0.005;  // ~1 event / 50 h per plant

/// Deterministic hash: (plant_id, epoch) → [0.0, 1.0).
/// Produces the same value for the same plant × time-window, ensuring a fault
/// event lasts the whole epoch and is reproducible across restarts.
#[inline]
fn det_hash(plant_id: &str, epoch: u64) -> f64 {
    let mut h: u64 = epoch
        .wrapping_mul(0x9e3779b97f4a7c15)
        .wrapping_add(0x6c62272e07bb0142);
    for b in plant_id.bytes() {
        h ^= (b as u64).wrapping_mul(0x517cc1b727220a95);
        h = h.rotate_left(17).wrapping_mul(0x0d2cb4c52a21f98d);
    }
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// Assigns fault_code only when no higher-priority code is already set.
/// Priority order: first-assigned wins (the triggering condition takes precedence).
#[inline]
fn try_set_fault(code: &mut u16, new_code: u16) {
    if *code == crate::models::power::alarm_codes::NONE { *code = new_code; }
}

#[derive(Clone, Debug)]
pub struct AppState {
    pub plant_data:     Arc<RwLock<HashMap<String, PlantData>>>,
    pub offline_mode:   Arc<AtomicBool>,
    pub mqtt_connected: Arc<AtomicBool>,
    /// Alarm registry: all alarms (active + historical)
    pub alarms:         Arc<RwLock<Vec<Alarm>>>,
    /// Event log ring-buffer
    pub events:         Arc<RwLock<VecDeque<Event>>>,
    /// Unix timestamp of when the process started (for uptime)
    pub start_time:     u64,
    /// Previous frequency per plant for ROCOF (Hz)
    prev_freq:          Arc<RwLock<HashMap<String, f64>>>,
}

impl AppState {
    pub fn new(offline_mode_default: bool) -> Self {
        let start = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            plant_data:     Arc::new(RwLock::new(HashMap::new())),
            offline_mode:   Arc::new(AtomicBool::new(offline_mode_default)),
            mqtt_connected: Arc::new(AtomicBool::new(false)),
            alarms:         Arc::new(RwLock::new(Vec::new())),
            events:         Arc::new(RwLock::new(VecDeque::new())),
            start_time:     start,
            prev_freq:      Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn is_offline(&self) -> bool {
        self.offline_mode.load(Ordering::Relaxed)
    }

    pub fn set_offline(&self, value: bool) {
        self.offline_mode.store(value, Ordering::Relaxed);
        self.push_event(None, EventKind::ModeChange, format!(
            "Mode changed to {}", if value { "OFFLINE" } else { "ONLINE" }
        ), None);
    }

    pub fn uptime_seconds(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now.saturating_sub(self.start_time)
    }

    // ── Alarm helpers ────────────────────────────────────────────────────────

    fn raise_alarm(&self, plant_id: &str, code: u16, severity: AlarmSeverity, message: &str) {
        let mut alarms = match self.alarms.write() { Ok(g) => g, Err(_) => return };
        // De-duplicate: don't raise the same active alarm twice
        if alarms.iter().any(|a| a.plant_id == plant_id && a.code == code && a.active) {
            return;
        }
        let id = uuid::Uuid::new_v4().to_string();
        alarms.push(Alarm {
            id:         id.clone(),
            plant_id:   plant_id.to_string(),
            code,
            severity:   severity.clone(),
            message:    message.to_string(),
            timestamp:  chrono::Utc::now(),
            active:     true,
            cleared_at: None,
        });
        // Trim history
        if alarms.len() > MAX_ALARM_HISTORY {
            alarms.remove(0);
        }
        drop(alarms);
        self.push_event(
            Some(plant_id.to_string()),
            EventKind::AlarmRaised,
            format!("[{:?}] {} — code {}", severity, message, code),
            None,
        );
    }

    fn clear_alarm(&self, plant_id: &str, code: u16) {
        let mut alarms = match self.alarms.write() { Ok(g) => g, Err(_) => return };
        let mut cleared = false;
        for a in alarms.iter_mut() {
            if a.plant_id == plant_id && a.code == code && a.active {
                a.active     = false;
                a.cleared_at = Some(chrono::Utc::now());
                cleared      = true;
            }
        }
        drop(alarms);
        if cleared {
            self.push_event(
                Some(plant_id.to_string()),
                EventKind::AlarmCleared,
                format!("Alarm code {} cleared", code),
                None,
            );
        }
    }

    pub fn push_event(
        &self,
        plant_id: Option<String>,
        kind: EventKind,
        message: String,
        payload: Option<serde_json::Value>,
    ) {
        let mut log = match self.events.write() { Ok(g) => g, Err(_) => return };
        log.push_front(Event {
            id:        uuid::Uuid::new_v4().to_string(),
            plant_id,
            kind,
            message,
            timestamp: chrono::Utc::now(),
            payload,
        });
        if log.len() > MAX_EVENT_LOG {
            log.pop_back();
        }
    }

    pub fn get_alarms(&self, plant_id: Option<&str>) -> Vec<Alarm> {
        let alarms = self.alarms.read().unwrap_or_else(|e| e.into_inner());
        match plant_id {
            Some(id) => alarms.iter().filter(|a| a.plant_id == id).cloned().collect(),
            None     => alarms.clone(),
        }
    }

    pub fn get_active_alarms(&self, plant_id: Option<&str>) -> Vec<Alarm> {
        self.get_alarms(plant_id).into_iter().filter(|a| a.active).collect()
    }

    pub fn get_events(&self, limit: usize) -> Vec<Event> {
        let log = self.events.read().unwrap_or_else(|e| e.into_inner());
        log.iter().take(limit).cloned().collect()
    }

    pub fn clear_plant_alarms(&self, plant_id: &str) {
        let mut alarms = match self.alarms.write() { Ok(g) => g, Err(_) => return };
        for a in alarms.iter_mut() {
            if a.plant_id == plant_id && a.active {
                a.active     = false;
                a.cleared_at = Some(chrono::Utc::now());
            }
        }
    }

    // ── Main data update ─────────────────────────────────────────────────────

    pub fn set_data(
        &self,
        plant_id: &str,
        dc_power: f64,          // raw DC power from solar algorithm (kW)
        temperature_c: f64,     // cell temperature (°C)
        ambient_temp_c: f64,    // ambient temperature (°C)
        nominal_power_kw: f64,
        weather_code: u16,
        is_day: bool,
        poa_irradiance_w_m2: f64,
        cloud_factor: f64,
        solar_elevation_deg: f64,
        wind_speed_m_s: f64,        // NEW: surface wind (m/s)
        relative_humidity_pct: f64, // NEW: relative humidity (%)
        soiling_factor: f64,        // NEW: panel soiling [0.85..1.0]
    ) {
        // ── 0. Timestamp for epoch-based fault injection ─────────────────────
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // ── 1. Retrieve or create entry ──────────────────────────────────────
        let mut map = match self.plant_data.write() { Ok(g) => g, Err(_) => return };
        let data = map.entry(plant_id.to_string()).or_default();

        data.weather_code          = weather_code;
        data.is_day                = is_day;
        data.poa_irradiance_w_m2   = poa_irradiance_w_m2;
        data.cloud_factor          = cloud_factor;
        data.solar_elevation_deg   = solar_elevation_deg;
        data.temperature_c         = temperature_c;
        data.ambient_temp_c        = ambient_temp_c;
        data.wind_speed_m_s        = wind_speed_m_s;
        data.relative_humidity_pct = relative_humidity_pct;
        data.soiling_factor        = soiling_factor;

        // ── 1b. Midnight daily-energy reset ──────────────────────────────────
        // Compare current day-of-year to last reset; reset at midnight.
        let today_doy = chrono::Utc::now().ordinal();
        if data.last_day_reset == 0 {
            // First run — initialise without clearing
            data.last_day_reset = today_doy;
        } else if data.last_day_reset != today_doy {
            data.daily_energy_kwh   = 0.0;
            data.daily_peak_power_kw = 0.0;
            data.last_day_reset     = today_doy;
        }

        // ── 2. MPPT startup / shutdown ramp ──────────────────────────────────
        // The inverter requires minimum irradiance before grid connection.
        // Below IRRAD_STOP_W_M2: ramp factor decays → shutdown.
        // Above IRRAD_START_W_M2: ramp factor grows → startup.
        // Power = dc_power × ramp_factor avoids abrupt steps.
        let ramp_target = if poa_irradiance_w_m2 >= IRRAD_START_W_M2 && is_day {
            1.0_f64
        } else if poa_irradiance_w_m2 < IRRAD_STOP_W_M2 {
            0.0_f64
        } else {
            // Hysteresis band: keep current ramp_factor
            data.ramp_factor
        };
        data.ramp_factor = (data.ramp_factor + (ramp_target - data.ramp_factor) * RAMP_RATE)
            .clamp(0.0, 1.0);
        let ramp = data.ramp_factor;

        // ── 2b. DC side: dual-MPPT string simulation ─────────────────────────
        // V_mpp tracks irradiance via temperature coefficient.
        let irr_ratio = (poa_irradiance_w_m2 / 1000.0).clamp(0.0, 1.1);
        data.mppt_voltage_v = V_DC_NOM * VMPP_VOC_RATIO
            * (1.0 + V_TEMP_COEFF * (temperature_c - 25.0));
        // DC bus: slightly above V_mpp during MPPT tracking
        data.dc_voltage_v  = data.mppt_voltage_v * 1.05;

        // Ramped DC power
        let dc_power_ramped = dc_power * ramp;
        data.dc_power_kw   = dc_power_ramped;
        data.dc_current_a  = if data.dc_voltage_v > 1.0 {
            dc_power_ramped * 1000.0 / data.dc_voltage_v
        } else { 0.0 };
        data.mppt_current_a = if data.mppt_voltage_v > 1.0 {
            dc_power_ramped * 1000.0 / data.mppt_voltage_v
        } else { 0.0 };

        // ── 2c. Dual-string imbalance ──────────────────────────────────────
        // String 1 carries ~50% + imbalance; string 2 the remainder.
        // Imbalance driven by:  shading mismatch + manufacturing spread.
        // Pseudo-random per plant × hour (shading stays stable ≥1 h).
        let str_epoch = now_secs / 3600;
        let h_str = det_hash(plant_id, str_epoch.wrapping_mul(23));
        // imbalance: ±8 % of total current (most common real-world deviation)
        let str_imb = (h_str * 2.0 - 1.0) * 0.08;
        let str1_frac = (0.50 + str_imb).clamp(0.30, 0.70);
        // Strings share the same MPPT bus voltage but carry different currents
        data.string1_voltage_v = data.mppt_voltage_v;
        data.string1_current_a = data.mppt_current_a * str1_frac * 2.0;
        data.string2_voltage_v = data.mppt_voltage_v;
        data.string2_current_a = data.mppt_current_a * (1.0 - str1_frac) * 2.0;

        // DC overvoltage check (panel V_oc can exceed MPPT range at cold temperatures)
        let v_oc_est = data.mppt_voltage_v / VMPP_VOC_RATIO;
        let dc_ov = v_oc_est > V_DC_NOM * 1.10; // >10% over rated

        // ── 3. Inverter efficiency curve (PV Inverter CEC model) ────────────
        let load_factor = if nominal_power_kw > 0.0 { dc_power_ramped / nominal_power_kw } else { 0.0 };
        let inv_eff = if load_factor < 0.01 {
            0.0
        } else if load_factor < 0.1 {
            0.80 + (load_factor / 0.1) * 0.155
        } else if load_factor < 0.5 {
            0.955 + ((load_factor - 0.1) / 0.4) * 0.025
        } else {
            0.980 - ((load_factor - 0.5) / 0.5) * 0.008
        };
        let temp_loss = (temperature_c - 25.0).max(0.0) * 0.0004;
        let efficiency = (inv_eff - temp_loss).clamp(0.0, 0.999);
        data.efficiency_percent = efficiency * 100.0;

        // ── 4. AC active power from DC through inverter ──────────────────────
        let ac_power = dc_power_ramped * efficiency;
        data.power_kw = ac_power;

        // ── 5. Inverter heatsink temperature (normalized first-order thermal model)
        // Steady-state: T_hs = T_amb + 20°C + loss_fraction × 65°C
        // loss_fraction = p_loss / P_nom_rated  → model scales for any plant size.
        // A 0.5% chance per 15-min epoch injects a "thermal event" (fan failure /
        // extreme ambient) that drives the heatsink above T_OVERTEMP_C.
        let p_loss        = dc_power_ramped - ac_power;
        let loss_fraction = if nominal_power_kw > 0.0 { p_loss / nominal_power_kw } else { 0.0 };
        let ot_epoch      = now_secs / 900;          // 15-minute windows
        let h_ot          = det_hash(plant_id, ot_epoch.wrapping_mul(17));
        let t_hs_target   = if h_ot < P_OT_FAULT && is_day {
            // Overtemperature event: fan failure / extreme heat → 85–95 °C
            T_OVERTEMP_C + 5.0 + (h_ot / P_OT_FAULT) * 10.0
        } else {
            ambient_temp_c + 20.0 + loss_fraction.clamp(0.0, 1.0) * 65.0
        };
        // First-order thermal filter τ ≈ 5 samples (25 s) — heatsink thermal mass
        data.inverter_temp_c = data.inverter_temp_c
            + (t_hs_target - data.inverter_temp_c) * 0.2;

        // ── 6. 3-phase AC voltage & frequency ─────────────────────────────
        // Epoch-based fault injection using det_hash:
        //  • 5-minute windows → faults last a whole epoch (realistic for grid events)
        //  • P_VOLT_FAULT (2.5%) chance per epoch for swell or sag
        //  • P_FREQ_FAULT (1.5%) chance per epoch for over/under-frequency
        // Normal operation stays firmly within EN 50160 limits (±4 V, ±0.08 Hz).
        let grid_epoch = now_secs / 300;   // 5-minute windows
        let h_swell    = det_hash(plant_id, grid_epoch.wrapping_mul(7));
        let h_sag      = det_hash(plant_id, grid_epoch.wrapping_mul(7) + 1);
        let h_freq_hi  = det_hash(plant_id, grid_epoch.wrapping_mul(7) + 2);
        let h_freq_lo  = det_hash(plant_id, grid_epoch.wrapping_mul(7) + 3);

        // Epoch-level voltage drift (slow, ±4 V — within EN 50160 normal band)
        let v_drift = (det_hash(plant_id, grid_epoch.wrapping_mul(7) + 4) * 2.0 - 1.0) * 4.0;
        // Per-sample fine ripple (±0.4 V — measurement noise)
        let h_rip = det_hash(plant_id, now_secs.wrapping_mul(11) ^ 0xA5A5);
        let v_ripple = (h_rip * 2.0 - 1.0) * 0.4;

        // Grid-event override: swell/sag pushes voltage well outside trip limits
        let v_offset = if h_swell < P_VOLT_FAULT {
            // Swell: +28..+46 V above nominal → clearly above V_OV_LIMIT (253 V)
            28.0 + (h_swell / P_VOLT_FAULT) * 18.0
        } else if h_sag < P_VOLT_FAULT {
            // Sag: −28..−46 V below nominal → clearly below V_UV_LIMIT (207 V)
            -(28.0 + (h_sag / P_VOLT_FAULT) * 18.0)
        } else {
            v_drift + v_ripple
        };

        // Realistic per-phase asymmetry (≤ ±0.5 V IEC 62052 class B)
        let h_ph  = det_hash(plant_id, now_secs ^ 0xCCCC);
        let h_ph2 = det_hash(plant_id, now_secs ^ 0xBEEF);
        data.voltage_l1_v = V_GRID_NOM + v_offset;
        data.voltage_l2_v = V_GRID_NOM + v_offset + (h_ph  * 2.0 - 1.0) * 0.5;
        data.voltage_l3_v = V_GRID_NOM + v_offset - (h_ph2 * 2.0 - 1.0) * 0.5;

        // Frequency: slow epoch-level oscillation ±0.08 Hz; fault events ±0.55 Hz
        let f_drift  = (det_hash(plant_id, grid_epoch.wrapping_mul(7) + 5) * 2.0 - 1.0) * 0.08;
        let h_frip   = det_hash(plant_id, now_secs.wrapping_mul(13) ^ 0xF0F0);
        let f_ripple = (h_frip * 2.0 - 1.0) * 0.01;
        let f_offset = if h_freq_hi < P_FREQ_FAULT {
            // Over-frequency event: +0.55..+0.80 Hz above F_NOM
            0.55 + (h_freq_hi / P_FREQ_FAULT) * 0.25
        } else if h_freq_lo < P_FREQ_FAULT {
            // Under-frequency event: −0.55..−0.80 Hz
            -(0.55 + (h_freq_lo / P_FREQ_FAULT) * 0.25)
        } else {
            f_drift + f_ripple
        };
        let new_freq = F_NOM + f_offset;

        // ROCOF: derivative of frequency between consecutive 5-second samples.
        // During epoch transitions (freq step) this will briefly spike — realistic.
        let prev_f = self.prev_freq.read()
            .map(|m| m.get(plant_id).copied().unwrap_or(new_freq))
            .unwrap_or(new_freq);
        data.rocof_hz_s = (new_freq - prev_f) / UPDATE_INTERVAL_S;
        data.frequency_hz = new_freq;
        if let Ok(mut pf) = self.prev_freq.write() {
            pf.insert(plant_id.to_string(), new_freq);
        }

        // ── 7. Power factor, apparent, reactive ──────────────────────────────
        if ac_power > 0.01 {
            let pf_base = 0.96 + 0.04 * (1.0 - (-12.0 * load_factor).exp());
            let pf_noise = (ac_power * 11.7).sin() * 0.004;
            data.power_factor   = (pf_base + pf_noise).clamp(0.80, 1.0);
        } else {
            data.power_factor   = 1.0;
        }
        data.apparent_power_kva = if data.power_factor > 0.0 { ac_power / data.power_factor } else { ac_power };
        let q_sq = data.apparent_power_kva.powi(2) - ac_power.powi(2);
        data.reactive_power_kvar = if q_sq > 0.0 { q_sq.sqrt() } else { 0.0 };

        // ── 7b. AC Total Harmonic Distortion (THD) ────────────────────────────
        // IEC 61727: THD < 5 % at rated power.
        // Pattern: high THD at very low load (>12%), decreases to ~1.8% at rated,
        // rises slightly above rated. Real IGBT inverters follow this profile.
        let thd_at_load = if load_factor < 0.02 {
            0.0 // no output → undefined, report 0
        } else if load_factor < 0.10 {
            12.0 - (load_factor / 0.10) * 7.5   // 12% down to 4.5% at 10% load
        } else if load_factor < 0.50 {
            4.5 - ((load_factor - 0.10) / 0.40) * 2.7  // 4.5% → 1.8% at 50% load
        } else {
            1.8 + ((load_factor - 0.50) / 0.50) * 0.5  // slight rise above 50%
        };
        // Fast per-cycle noise (±0.2 %) from switching ripple
        let h_thd = det_hash(plant_id, now_secs.wrapping_mul(31) ^ 0x55AA);
        data.ac_thd_percent = (thd_at_load + (h_thd * 2.0 - 1.0) * 0.2).max(0.0);

        // ── 7c. DC injection into AC grid ──────────────────────────────────
        // IEEE 1547 / IEC 61727: limit 0.5% of rated AC current.
        // Model: 0.05–0.5 % of I_rated depending on load and high-frequency noise;
        //        epoch-based to keep it stable within one cycle.
        let dc_inj_epoch = now_secs / 60; // 1-minute windows
        let h_dc_inj = det_hash(plant_id, dc_inj_epoch.wrapping_mul(19));
        let i_rated_a = if V_GRID_NOM > 0.0 { nominal_power_kw * 1000.0 / (3.0 * V_GRID_NOM) } else { 0.0 };
        data.dc_injection_ma = if ac_power > 0.01 {
            i_rated_a * (0.05 + h_dc_inj * 0.45) / 100.0 * 1000.0 // 0.05–0.5 % in mA
        } else { 0.0 };

        // ── 8. Phase currents (balanced 3-phase split) ───────────────────────
        let phase_va = data.apparent_power_kva * 1000.0 / 3.0;
        data.current_l1_a = if data.voltage_l1_v > 0.0 { phase_va / data.voltage_l1_v } else { 0.0 };
        data.current_l2_a = if data.voltage_l2_v > 0.0 { phase_va / data.voltage_l2_v } else { 0.0 };
        data.current_l3_a = if data.voltage_l3_v > 0.0 { phase_va / data.voltage_l3_v } else { 0.0 };

        // ── 9. Isolation resistance (DC-GND, three-layer model) ───────────────
        // a) Normal 10–40 MΩ, highest at midday (dry, warm panels)
        // b) Dawn-dew effect: panels cold and wet → reduced isolation at low elevation
        // c) "Wet day" fault event (P_ISOL_FAULT per hour epoch): < 0.4 MΩ → trip
        let isol_epoch = now_secs / 3600;   // 1-hour windows
        let h_wet      = det_hash(plant_id, isol_epoch.wrapping_mul(13));
        let isol_base  = if h_wet < P_ISOL_FAULT && is_day {
            // Isolation fault (heavy rain, panel surface contamination)
            0.05 + (h_wet / P_ISOL_FAULT) * 0.35   // 0.05 – 0.40 MΩ (below 0.5 MΩ threshold)
        } else {
            10.0 + irr_ratio * 30.0   // 10–40 MΩ scaling with irradiance
        };
        // Dawn-dew factor: isolation recovers linearly as panels warm (0–20° elevation)
        let dew_factor = if is_day && solar_elevation_deg < 20.0 {
            0.30 + (solar_elevation_deg / 20.0) * 0.70   // 0.30 at horizon → 1.0 at 20°
        } else {
            1.0
        };
        data.isolation_resistance_mohm = (isol_base * dew_factor).max(0.05);

        // ── 9b. Leakage (residual) current to ground (mA) ────────────────────
        // Model: IEC 62109 — normal < 50 mA; concern zone 50–300 mA; trip > 300 mA.
        // Higher with humidity (moisture on panel frames / cabling).
        // More leakage when isolation resistance is low.
        let leak_humidity_factor = 1.0 + (relative_humidity_pct - 50.0).max(0.0) * 0.012;
        // Base leakage ∝ (1 / isolation) scaled to realistic range
        let leak_base = (1.0 / data.isolation_resistance_mohm.max(0.01)) * 2.5 * leak_humidity_factor;
        let h_leak = det_hash(plant_id, now_secs.wrapping_mul(43) ^ 0x1234);
        data.leakage_current_ma = (leak_base + h_leak * 0.5).clamp(0.05, 350.0);

        // ── 9c. Inverter cooling fan model ────────────────────────────────────
        // Real inverters: fan off below 40°C heatsink, variable 1500–3600 RPM above.
        // Fan fault: injected with P_FAN_FAULT probability per 4-hour epoch.
        const P_FAN_FAULT: f64 = 0.008; // ~1 event per 500 h per plant
        let fan_epoch = now_secs / 14400; // 4-hour windows
        let h_fan = det_hash(plant_id, fan_epoch.wrapping_mul(29));
        let fan_fail = h_fan < P_FAN_FAULT && is_day;
        data.fan_fault_active = fan_fail;

        let fan_rpm = if data.inverter_temp_c < 40.0 {
            0u16
        } else if fan_fail {
            // Fan fault: rotor stall → 0 RPM (no cooling → temperature runaway)
            0
        } else {
            // Linear 1500 – 3600 RPM across 40–80°C
            let frac = ((data.inverter_temp_c - 40.0) / 40.0).clamp(0.0, 1.0);
            (1500.0 + frac * 2100.0) as u16
        };
        data.inverter_fan_speed_rpm = fan_rpm;

        // ── 10. Status determination ─────────────────────────────────────────
        let v_avg = (data.voltage_l1_v + data.voltage_l2_v + data.voltage_l3_v) / 3.0;
        let has_fault = v_avg > V_OV_LIMIT || v_avg < V_UV_LIMIT
            || data.frequency_hz > F_OV_LIMIT || data.frequency_hz < F_UV_LIMIT
            || data.rocof_hz_s.abs() > ROCOF_LIMIT
            || data.isolation_resistance_mohm < ISOL_FAULT_MOHM
            || data.inverter_temp_c > T_OVERTEMP_C
            || (data.fan_fault_active && data.inverter_temp_c > T_OVERTEMP_C - 5.0)
            || dc_ov;

        data.status = if has_fault {
            2  // Fault
        } else if ramp < 0.05 && poa_irradiance_w_m2 < IRRAD_START_W_M2 {
            0  // Stopped / night
        } else if ramp < 0.99 && poa_irradiance_w_m2 >= IRRAD_START_W_M2 {
            4  // Starting (ramp-up in progress)
        } else if ramp > 0.0 && ramp < 1.0 && poa_irradiance_w_m2 < IRRAD_START_W_M2 {
            3  // Curtailed / shutting down (ramp-down in progress)
        } else if ac_power > 0.001 {
            if load_factor < 0.999 { 5 } else { 1 }  // 5=MPPT tracking, 1=Running at rated
        } else if is_day && solar_elevation_deg > 1.0 {
            4  // Starting (waiting for irradiance)
        } else {
            0  // Stopped (night)
        };

        // ── 11. Alarm / fault code logic ────────────────────────────────────
        // Snapshot fields needed for alarm logic (before releasing write lock)
        let snap_freq     = data.frequency_hz;
        let snap_isol     = data.isolation_resistance_mohm;
        let snap_inv_temp = data.inverter_temp_c;
        let snap_rocof    = data.rocof_hz_s;
        let snap_leak     = data.leakage_current_ma;
        let snap_fan_fail = data.fan_fault_active;
        let snap_fan_rpm  = data.inverter_fan_speed_rpm;

        let mut new_flags: u32 = 0;
        let mut fault_code: u16 = alarm_codes::NONE;

        drop(map); // release write lock before calling alarm helpers

        // Overvoltage
        if v_avg > V_OV_LIMIT {
            new_flags |= alarm_flag_bits::AC_OVERVOLTAGE;
            try_set_fault(&mut fault_code, alarm_codes::AC_OVERVOLTAGE);
            self.raise_alarm(plant_id, alarm_codes::AC_OVERVOLTAGE, AlarmSeverity::Warning,
                &format!("AC overvoltage: {:.1} V (limit {:.0} V)", v_avg, V_OV_LIMIT));
        } else { self.clear_alarm(plant_id, alarm_codes::AC_OVERVOLTAGE); }

        // Undervoltage
        if v_avg < V_UV_LIMIT && is_day {
            new_flags |= alarm_flag_bits::AC_UNDERVOLTAGE;
            try_set_fault(&mut fault_code, alarm_codes::AC_UNDERVOLTAGE);
            self.raise_alarm(plant_id, alarm_codes::AC_UNDERVOLTAGE, AlarmSeverity::Warning,
                &format!("AC undervoltage: {:.1} V (limit {:.0} V)", v_avg, V_UV_LIMIT));
        } else { self.clear_alarm(plant_id, alarm_codes::AC_UNDERVOLTAGE); }

        // Frequency — distinguish over-frequency from under-frequency
        if snap_freq > F_OV_LIMIT {
            new_flags |= alarm_flag_bits::FREQUENCY_FAULT;
            try_set_fault(&mut fault_code, alarm_codes::AC_OVERFREQUENCY);
            self.raise_alarm(plant_id, alarm_codes::AC_OVERFREQUENCY, AlarmSeverity::Warning,
                &format!("Over-frequency: {:.3} Hz (limit {:.2} Hz)", snap_freq, F_OV_LIMIT));
            self.clear_alarm(plant_id, alarm_codes::AC_UNDERFREQUENCY);
        } else if snap_freq < F_UV_LIMIT {
            new_flags |= alarm_flag_bits::FREQUENCY_FAULT;
            try_set_fault(&mut fault_code, alarm_codes::AC_UNDERFREQUENCY);
            self.raise_alarm(plant_id, alarm_codes::AC_UNDERFREQUENCY, AlarmSeverity::Warning,
                &format!("Under-frequency: {:.3} Hz (limit {:.2} Hz)", snap_freq, F_UV_LIMIT));
            self.clear_alarm(plant_id, alarm_codes::AC_OVERFREQUENCY);
        } else {
            self.clear_alarm(plant_id, alarm_codes::AC_OVERFREQUENCY);
            self.clear_alarm(plant_id, alarm_codes::AC_UNDERFREQUENCY);
        }

        // Isolation fault
        if snap_isol < ISOL_FAULT_MOHM {
            new_flags |= alarm_flag_bits::ISOLATION_FAULT;
            try_set_fault(&mut fault_code, alarm_codes::ISOLATION_FAULT);
            self.raise_alarm(plant_id, alarm_codes::ISOLATION_FAULT, AlarmSeverity::Fault,
                &format!("Isolation resistance too low: {:.2} MΩ (limit {:.1} MΩ)", snap_isol, ISOL_FAULT_MOHM));
        } else { self.clear_alarm(plant_id, alarm_codes::ISOLATION_FAULT); }

        // Leakage current (IEC 62109 limit 300 mA — Critical; 100 mA — Warning)
        if snap_leak > 300.0 {
            new_flags |= alarm_flag_bits::LEAKAGE_CURRENT;
            try_set_fault(&mut fault_code, alarm_codes::GROUND_FAULT);
            self.raise_alarm(plant_id, alarm_codes::GROUND_FAULT, AlarmSeverity::Critical,
                &format!("Leakage current critical: {:.1} mA (trip >300 mA)", snap_leak));
        } else if snap_leak > 100.0 {
            new_flags |= alarm_flag_bits::LEAKAGE_CURRENT;
            self.raise_alarm(plant_id, alarm_codes::GROUND_FAULT, AlarmSeverity::Warning,
                &format!("Leakage current elevated: {:.1} mA (warn >100 mA)", snap_leak));
        } else { self.clear_alarm(plant_id, alarm_codes::GROUND_FAULT); }

        // Overtemperature
        if snap_inv_temp > T_OVERTEMP_C {
            new_flags |= alarm_flag_bits::OVERTEMPERATURE;
            try_set_fault(&mut fault_code, alarm_codes::OVERTEMPERATURE);
            self.raise_alarm(plant_id, alarm_codes::OVERTEMPERATURE, AlarmSeverity::Critical,
                &format!("Inverter overtemperature: {:.1} °C (limit {:.0} °C)", snap_inv_temp, T_OVERTEMP_C));
        } else { self.clear_alarm(plant_id, alarm_codes::OVERTEMPERATURE); }

        // Fan fault (fan stopped while inverter is hot)
        if snap_fan_fail && snap_inv_temp > 45.0 {
            new_flags |= alarm_flag_bits::FAN_FAULT;
            try_set_fault(&mut fault_code, alarm_codes::FAN_FAULT);
            self.raise_alarm(plant_id, alarm_codes::FAN_FAULT, AlarmSeverity::Warning,
                &format!("Cooling fan fault: 0 RPM at {:.1} °C heatsink", snap_inv_temp));
        } else {
            // Fan running — check for under-speed (e.g. partial stall)
            if ac_power > 0.1 && snap_fan_rpm > 0 && snap_fan_rpm < 1200 && snap_inv_temp > 50.0 {
                new_flags |= alarm_flag_bits::FAN_FAULT;
                self.raise_alarm(plant_id, alarm_codes::FAN_FAULT, AlarmSeverity::Warning,
                    &format!("Fan under-speed: {} RPM (expected ≥1500 RPM)", snap_fan_rpm));
            } else {
                self.clear_alarm(plant_id, alarm_codes::FAN_FAULT);
            }
        }

        // DC overvoltage
        if dc_ov {
            new_flags |= alarm_flag_bits::DC_OVERVOLTAGE;
            try_set_fault(&mut fault_code, alarm_codes::DC_OVERVOLTAGE);
            self.raise_alarm(plant_id, alarm_codes::DC_OVERVOLTAGE, AlarmSeverity::Warning,
                &format!("DC string over-voltage: estimated V_oc > {:.0} V rated DC bus", V_DC_NOM));
        } else { self.clear_alarm(plant_id, alarm_codes::DC_OVERVOLTAGE); }

        // ROCOF — measured frequency derivative between 5-second samples
        if snap_rocof.abs() > ROCOF_LIMIT {
            new_flags |= alarm_flag_bits::ROCOF_TRIP;
            try_set_fault(&mut fault_code, alarm_codes::ROCOF_TRIP);
            self.raise_alarm(plant_id, alarm_codes::ROCOF_TRIP, AlarmSeverity::Critical,
                &format!("RoCoF trip: {:.3} Hz/s (limit ±{:.1} Hz/s)", snap_rocof, ROCOF_LIMIT));
        } else { self.clear_alarm(plant_id, alarm_codes::ROCOF_TRIP); }

        // Write alarm flags back
        let mut map2 = match self.plant_data.write() { Ok(g) => g, Err(_) => return };
        if let Some(d) = map2.get_mut(plant_id) {
            d.fault_code  = fault_code;
            d.alarm_flags = new_flags;

            // ── 12. Energy accounting ────────────────────────────────────────
            let kwh_per_sample = d.power_kw * (UPDATE_INTERVAL_S / 3600.0);
            d.daily_energy_kwh   += kwh_per_sample;
            d.monthly_energy_kwh += kwh_per_sample;
            d.total_energy_kwh   += kwh_per_sample;

            // CO₂ avoided: ENTSO-E European grid average ≈ 0.233 kg CO₂/kWh
            d.co2_avoided_kg += kwh_per_sample * 0.233;

            // Today's peak AC power
            if d.power_kw > d.daily_peak_power_kw {
                d.daily_peak_power_kw = d.power_kw;
            }

            // ── 13. Performance KPIs ─────────────────────────────────────────
            // PR = actual yield / reference yield;  ref yield = G_poa/1000 * P_nom
            let ref_yield = (d.poa_irradiance_w_m2 / 1000.0) * nominal_power_kw;
            d.performance_ratio = if ref_yield > 0.1 {
                (d.power_kw / ref_yield).clamp(0.0, 1.0)
            } else { 0.0 };

            d.specific_yield_kwh_kwp = if nominal_power_kw > 0.0 {
                d.daily_energy_kwh / nominal_power_kw
            } else { 0.0 };

            d.capacity_factor_percent = if nominal_power_kw > 0.0 {
                (d.power_kw / nominal_power_kw * 100.0).clamp(0.0, 110.0)
            } else { 0.0 };

            #[cfg(feature = "verbose_log")]
            println!(
                "[UPDATE] {} | AC {:.2} kW | DC {:.2} kW | eff {:.1}% | L1 {:.1}V | T_inv {:.1}°C | PR {:.2} | flags 0x{:04X}",
                plant_id, d.power_kw, d.dc_power_kw, d.efficiency_percent,
                d.voltage_l1_v, d.inverter_temp_c, d.performance_ratio, d.alarm_flags
            );
        }
    }

    pub fn get_data(&self, plant_id: &str) -> Option<PlantData> {
        self.plant_data.read().ok()?.get(plant_id).cloned()
    }

    pub fn get_all_data(&self) -> HashMap<String, PlantData> {
        self.plant_data.read()
            .map(|m| m.clone())
            .unwrap_or_default()
    }
}

// ─── A simple uptime counter that auto-increments (for future use) ───────────
#[allow(dead_code)]
pub struct Counter(Arc<AtomicU64>);
impl Counter {
    pub fn new() -> Self { Counter(Arc::new(AtomicU64::new(0))) }
    pub fn inc(&self) { self.0.fetch_add(1, Ordering::Relaxed); }
    pub fn value(&self) -> u64 { self.0.load(Ordering::Relaxed) }
}

// ─── Combined Axum state ─────────────────────────────────────────────────────
/// Holds both AppState and Config so that Axum handlers may extract either
/// via `State<AppState>` or `State<Config>` using the `FromRef` trait.
#[derive(Clone)]
pub struct SharedState {
    pub app:    AppState,
    pub config: crate::config::Config,
}

impl axum::extract::FromRef<SharedState> for AppState {
    fn from_ref(s: &SharedState) -> AppState { s.app.clone() }
}

impl axum::extract::FromRef<SharedState> for crate::config::Config {
    fn from_ref(s: &SharedState) -> crate::config::Config { s.config.clone() }
}

