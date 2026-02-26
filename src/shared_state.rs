use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

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
    ) {
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

        // ── 2. DC side: MPPT simulation ──────────────────────────────────────
        // V_mpp tracks irradiance: high irradiance → near rated voltage
        let irr_ratio = (poa_irradiance_w_m2 / 1000.0).clamp(0.0, 1.1);
        // V_mpp = V_dc_nom * (VMPP/VOC) * (1 − α_v*(T−25)) roughly
        let v_temp_coeff = -0.0035; // V/°C/cell for c-Si
        data.mppt_voltage_v = V_DC_NOM * VMPP_VOC_RATIO * (1.0 + v_temp_coeff * (temperature_c - 25.0));
        // V_dc bus is slightly above V_mpp during MPPT tracking
        data.dc_voltage_v = data.mppt_voltage_v * 1.05;
        // DC power = what the solar algorithm says
        data.dc_power_kw  = dc_power;
        data.dc_current_a = if data.dc_voltage_v > 1.0 { dc_power * 1000.0 / data.dc_voltage_v } else { 0.0 };
        data.mppt_current_a = if data.mppt_voltage_v > 1.0 { dc_power * 1000.0 / data.mppt_voltage_v } else { 0.0 };

        // ── 3. Inverter efficiency curve (PV Inverter CEC model) ────────────
        let load_factor = if nominal_power_kw > 0.0 { dc_power / nominal_power_kw } else { 0.0 };
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
        let ac_power = dc_power * efficiency;
        data.power_kw = ac_power;

        // ── 5. Inverter heatsink temperature ────────────────────────────────
        // T_hs = T_amb + k * P_loss  (thermal model, k ≈ 8 °C/kW loss)
        let p_loss = dc_power - ac_power;
        data.inverter_temp_c = ambient_temp_c + 15.0 + p_loss * 8.0 * irr_ratio;

        // ── 6. 3-phase AC voltage & frequency ─────────────────────────────
        // Small asymmetry between phases (realistic noise ±0.3%)
        let noise = |seed: f64| -> f64 { (seed * ac_power + 1.0).sin() * 0.7 };
        data.voltage_l1_v = V_GRID_NOM + noise(11.3);
        data.voltage_l2_v = V_GRID_NOM + noise(17.7);
        data.voltage_l3_v = V_GRID_NOM + noise(23.1);

        let freq_noise = if ac_power > 0.01 { (ac_power * 7.3).cos() * 0.04 } else { 0.0 };
        let new_freq = F_NOM + freq_noise;
        // ROCOF from previous sample
        let prev_f = self.prev_freq.read()
            .map(|m| m.get(plant_id).copied().unwrap_or(F_NOM))
            .unwrap_or(F_NOM);
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

        // ── 8. Phase currents (balanced 3-phase split) ───────────────────────
        let phase_va = data.apparent_power_kva * 1000.0 / 3.0;
        data.current_l1_a = if data.voltage_l1_v > 0.0 { phase_va / data.voltage_l1_v } else { 0.0 };
        data.current_l2_a = if data.voltage_l2_v > 0.0 { phase_va / data.voltage_l2_v } else { 0.0 };
        data.current_l3_a = if data.voltage_l3_v > 0.0 { phase_va / data.voltage_l3_v } else { 0.0 };

        // ── 9. Isolation resistance (higher when wet / morning dew) ──────────
        // Simulates natural variation: lower at dawn, higher midday
        let isol_factor = if irr_ratio > 0.05 { 1.0 + irr_ratio * 3.0 } else { 0.5 };
        data.isolation_resistance_mohm = (10.0 * isol_factor).min(50.0);

        // ── 10. Status determination ─────────────────────────────────────────
        let v_avg = (data.voltage_l1_v + data.voltage_l2_v + data.voltage_l3_v) / 3.0;
        let has_fault = v_avg > V_OV_LIMIT || v_avg < V_UV_LIMIT
            || data.frequency_hz > F_OV_LIMIT || data.frequency_hz < F_UV_LIMIT
            || data.rocof_hz_s.abs() > ROCOF_LIMIT
            || data.isolation_resistance_mohm < ISOL_FAULT_MOHM
            || data.inverter_temp_c > T_OVERTEMP_C;

        data.status = if has_fault {
            2  // Fault
        } else if ac_power > 0.001 {
            if load_factor < 0.999 { 5 } else { 1 }  // 5=MPPT tracking, 1=Running
        } else if is_day && solar_elevation_deg > 2.0 {
            4  // Starting (low irradiance but daytime)
        } else {
            0  // Stopped (night)
        };

        // ── 11. Alarm / fault code logic ────────────────────────────────────
        // Snapshot fields needed for alarm logic (before releasing write lock)
        let snap_freq     = data.frequency_hz;
        let snap_isol     = data.isolation_resistance_mohm;
        let snap_inv_temp = data.inverter_temp_c;
        let snap_rocof    = data.rocof_hz_s;

        let mut new_flags: u32 = 0;
        let mut fault_code: u16 = alarm_codes::NONE;

        drop(map); // release write lock before calling alarm helpers

        // Overvoltage
        let ov = v_avg > V_OV_LIMIT;
        if ov { new_flags |= alarm_flag_bits::AC_OVERVOLTAGE; fault_code = alarm_codes::AC_OVERVOLTAGE;
            self.raise_alarm(plant_id, alarm_codes::AC_OVERVOLTAGE, AlarmSeverity::Warning,
                &format!("AC overvoltage: {:.1} V (limit {:.0} V)", v_avg, V_OV_LIMIT));
        } else { self.clear_alarm(plant_id, alarm_codes::AC_OVERVOLTAGE); }

        // Undervoltage
        let uv = v_avg < V_UV_LIMIT && is_day;
        if uv { new_flags |= alarm_flag_bits::AC_UNDERVOLTAGE; fault_code = alarm_codes::AC_UNDERVOLTAGE;
            self.raise_alarm(plant_id, alarm_codes::AC_UNDERVOLTAGE, AlarmSeverity::Warning,
                &format!("AC undervoltage: {:.1} V (limit {:.0} V)", v_avg, V_UV_LIMIT));
        } else { self.clear_alarm(plant_id, alarm_codes::AC_UNDERVOLTAGE); }

        // Frequency
        let freq_fault = snap_freq < F_UV_LIMIT || snap_freq > F_OV_LIMIT;
        if freq_fault { new_flags |= alarm_flag_bits::FREQUENCY_FAULT; fault_code = alarm_codes::AC_OVERFREQUENCY;
            self.raise_alarm(plant_id, alarm_codes::AC_OVERFREQUENCY, AlarmSeverity::Warning,
                &format!("Frequency out of range: {:.3} Hz", snap_freq));
        } else { self.clear_alarm(plant_id, alarm_codes::AC_OVERFREQUENCY);
                  self.clear_alarm(plant_id, alarm_codes::AC_UNDERFREQUENCY); }

        // Isolation fault
        let isol_fault = snap_isol < ISOL_FAULT_MOHM;
        if isol_fault { new_flags |= alarm_flag_bits::ISOLATION_FAULT; fault_code = alarm_codes::ISOLATION_FAULT;
            self.raise_alarm(plant_id, alarm_codes::ISOLATION_FAULT, AlarmSeverity::Fault,
                &format!("Isolation resistance too low: {:.2} MΩ", snap_isol));
        } else { self.clear_alarm(plant_id, alarm_codes::ISOLATION_FAULT); }

        // Overtemperature
        let overtemp = snap_inv_temp > T_OVERTEMP_C;
        if overtemp { new_flags |= alarm_flag_bits::OVERTEMPERATURE; fault_code = alarm_codes::OVERTEMPERATURE;
            self.raise_alarm(plant_id, alarm_codes::OVERTEMPERATURE, AlarmSeverity::Critical,
                &format!("Inverter overtemperature: {:.1} °C", snap_inv_temp));
        } else { self.clear_alarm(plant_id, alarm_codes::OVERTEMPERATURE); }

        // ROCOF
        if snap_rocof.abs() > ROCOF_LIMIT {
            self.raise_alarm(plant_id, alarm_codes::ROCOF_TRIP, AlarmSeverity::Critical,
                &format!("RoCoF trip: {:.3} Hz/s", snap_rocof));
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

