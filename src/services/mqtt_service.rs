/// MQTT telemetry publisher
///
/// Publishes plant telemetry as JSON payloads to a configured MQTT broker.
/// Topic structure: `{prefix}/{plant_id}/telemetry`
/// Also publishes system-wide summary: `{prefix}/system/summary`
///
/// Standard-compatible: payloads follow the Sparkplug B field naming convention
/// where possible, but serialised as plain JSON for maximum compatibility.

use std::time::Duration;
use rumqttc::{AsyncClient, MqttOptions, QoS};
use crate::config::MqttConfig;
use crate::shared_state::AppState;
use crate::config::PlantConfig;

pub async fn run_publisher(
    cfg: MqttConfig,
    state: AppState,
    plants: Vec<PlantConfig>,
) {
    if !cfg.enabled || cfg.broker_host.is_empty() {
        println!("[MQTT] Disabled or no broker configured — skipping MQTT publisher");
        return;
    }

    let client_id = if cfg.client_id.is_empty() {
        format!("solar-scada-{}", uuid::Uuid::new_v4())
    } else {
        cfg.client_id.clone()
    };

    let interval_s = cfg.publish_interval_s.unwrap_or(10).max(1);
    let prefix     = cfg.topic_prefix.trim_end_matches('/').to_string();

    println!(
        "[MQTT] Connecting to {}:{} (client_id={}, interval={}s)",
        cfg.broker_host, cfg.broker_port, client_id, interval_s
    );

    let mut opts = MqttOptions::new(&client_id, &cfg.broker_host, cfg.broker_port);
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_clean_session(true);

    if let (Some(user), Some(pass)) = (&cfg.username, &cfg.password) {
        opts.set_credentials(user, pass);
    }

    let (client, mut eventloop) = AsyncClient::new(opts, 64);

    // Publish birth message
    let birth_topic = format!("{}/system/status", prefix);
    let birth_payload = serde_json::json!({
        "status": "ONLINE",
        "version": env!("CARGO_PKG_VERSION"),
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });
    if let Err(e) = client.publish(
        &birth_topic,
        QoS::AtLeastOnce,
        true, // retained
        birth_payload.to_string().as_bytes(),
    ).await {
        eprintln!("[MQTT] Failed to publish birth message: {}", e);
    } else {
        state.mqtt_connected.store(true, std::sync::atomic::Ordering::Relaxed);
        println!("[MQTT] Connected, birth message published to {}", birth_topic);
    }

    // Will message topic (set before connect — for next reconnect cycle)
    let _will_topic  = format!("{}/system/status", prefix);
    let will_payload = serde_json::json!({ "status": "OFFLINE" });
    drop(will_payload); // MqttOptions::set_last_will could be set before creating client

    loop {
        // Drain event loop without blocking the publish loop
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(interval_s)) => {}
            event = eventloop.poll() => {
                match event {
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("[MQTT] Event loop error: {} — will reconnect", e);
                        state.mqtt_connected.store(false, std::sync::atomic::Ordering::Relaxed);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
                continue;
            }
        }

        // Publish per-plant telemetry
        for plant in &plants {
            if let Some(data) = state.get_data(&plant.id) {
                let status_label = match data.status {
                    1 => "RUNNING", 2 => "FAULT", 3 => "CURTAILED",
                    4 => "STARTING", 5 => "MPPT", _ => "STOPPED",
                };
                let payload = serde_json::json!({
                    // Identity
                    "plant_id":   plant.id,
                    "plant_name": plant.name,
                    "timestamp":  chrono::Utc::now().to_rfc3339(),
                    // AC Output
                    "ac": {
                        "power_kw":           data.power_kw,
                        "voltage_l1_v":       data.voltage_l1_v,
                        "voltage_l2_v":       data.voltage_l2_v,
                        "voltage_l3_v":       data.voltage_l3_v,
                        "current_l1_a":       data.current_l1_a,
                        "current_l2_a":       data.current_l2_a,
                        "current_l3_a":       data.current_l3_a,
                        "frequency_hz":       data.frequency_hz,
                        "rocof_hz_s":         data.rocof_hz_s,
                        "power_factor":       data.power_factor,
                        "reactive_kvar":      data.reactive_power_kvar,
                        "apparent_kva":       data.apparent_power_kva,
                    },
                    // DC / MPPT
                    "dc": {
                        "voltage_v":          data.dc_voltage_v,
                        "current_a":          data.dc_current_a,
                        "power_kw":           data.dc_power_kw,
                        "mppt_voltage_v":     data.mppt_voltage_v,
                        "mppt_current_a":     data.mppt_current_a,
                    },
                    // Thermal
                    "thermal": {
                        "cell_temp_c":        data.temperature_c,
                        "inverter_temp_c":    data.inverter_temp_c,
                        "ambient_temp_c":     data.ambient_temp_c,
                    },
                    // Irradiance
                    "irradiance": {
                        "poa_w_m2":           data.poa_irradiance_w_m2,
                        "cloud_factor":       data.cloud_factor,
                        "solar_elevation_deg": data.solar_elevation_deg,
                    },
                    // Status & protection
                    "status": status_label,
                    "fault_code":             data.fault_code,
                    "alarm_flags":            data.alarm_flags,
                    "isolation_resistance_mohm": data.isolation_resistance_mohm,
                    // Energy
                    "energy": {
                        "daily_kwh":          data.daily_energy_kwh,
                        "monthly_kwh":        data.monthly_energy_kwh,
                        "total_kwh":          data.total_energy_kwh,
                    },
                    // KPIs
                    "kpi": {
                        "efficiency_percent":     data.efficiency_percent,
                        "performance_ratio":      data.performance_ratio,
                        "specific_yield_kwh_kwp": data.specific_yield_kwh_kwp,
                        "capacity_factor_percent": data.capacity_factor_percent,
                    },
                    // Weather
                    "weather_code": data.weather_code,
                    "is_day":       data.is_day,
                });

                let topic = format!("{}/{}/telemetry", prefix, plant.id);
                if let Err(e) = client.publish(
                    &topic,
                    QoS::AtMostOnce,
                    false,
                    payload.to_string().as_bytes(),
                ).await {
                    eprintln!("[MQTT] Publish error for {}: {}", topic, e);
                    state.mqtt_connected.store(false, std::sync::atomic::Ordering::Relaxed);
                } else {
                    state.mqtt_connected.store(true, std::sync::atomic::Ordering::Relaxed);
                }

                // Also publish alarms topic if any active alarms
                let active_alarms = state.get_active_alarms(Some(&plant.id));
                if !active_alarms.is_empty() {
                    let alarm_topic = format!("{}/{}/alarms", prefix, plant.id);
                    let alarm_payload = serde_json::to_string(&active_alarms).unwrap_or_default();
                    let _ = client.publish(&alarm_topic, QoS::AtLeastOnce, true, alarm_payload.as_bytes()).await;
                }
            }
        }

        // Publish fleet summary
        let all_data  = state.get_all_data();
        let total_kw  : f64 = all_data.values().map(|d| d.power_kw).sum();
        let total_kwh : f64 = all_data.values().map(|d| d.daily_energy_kwh).sum();
        let total_nom : f64 = plants.iter().map(|p| p.nominal_power_kw).sum();
        let running   = all_data.values().filter(|d| d.status == 1 || d.status == 5).count();
        let fleet_pr  : f64 = if !all_data.is_empty() {
            all_data.values().map(|d| d.performance_ratio).sum::<f64>() / all_data.len() as f64
        } else { 0.0 };

        let summary = serde_json::json!({
            "timestamp":            chrono::Utc::now().to_rfc3339(),
            "total_power_kw":       total_kw,
            "total_nominal_kw":     total_nom,
            "total_daily_kwh":      total_kwh,
            "plants_running":       running,
            "plants_total":         plants.len(),
            "fleet_pr":             fleet_pr,
            "offline_mode":         state.is_offline(),
        });

        let summary_topic = format!("{}/system/summary", prefix);
        let _ = client.publish(
            &summary_topic,
            QoS::AtMostOnce,
            false,
            summary.to_string().as_bytes(),
        ).await;
    }
}
