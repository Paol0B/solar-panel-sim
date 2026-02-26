use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use tokio_modbus::prelude::*;
use tokio_modbus::server::Service;
use tokio_modbus::ExceptionCode;

use crate::shared_state::AppState;

// ─── Register offset constants (relative to plant base_address) ──────────────
// All float32 variables occupy TWO consecutive u16 registers (IEEE 754 big-endian:
// high word at base+offset, low word at base+offset+1).
// u16 variables occupy ONE register.
//
// Recommended block size: 100 registers per plant.

/// AC Output — Power & Grid
pub const REG_POWER_KW:            u16 =  0;  // float32  kW
pub const REG_VOLTAGE_L1_V:        u16 =  2;  // float32  V
pub const REG_CURRENT_L1_A:        u16 =  4;  // float32  A
pub const REG_FREQUENCY_HZ:        u16 =  6;  // float32  Hz
pub const REG_TEMPERATURE_C:       u16 =  8;  // float32  °C  (cell)
pub const REG_STATUS:              u16 = 10;  // u16      enum 0-5
pub const REG_VOLTAGE_L2_V:        u16 = 11;  // float32  V
pub const REG_VOLTAGE_L3_V:        u16 = 13;  // float32  V
pub const REG_CURRENT_L2_A:        u16 = 15;  // float32  A
pub const REG_CURRENT_L3_A:        u16 = 17;  // float32  A
pub const REG_REACTIVE_POWER_KVAR: u16 = 19;  // float32  kvar
pub const REG_APPARENT_POWER_KVA:  u16 = 21;  // float32  kVA
pub const REG_POWER_FACTOR:        u16 = 23;  // float32  —
pub const REG_ROCOF_HZ_S:          u16 = 25;  // float32  Hz/s

/// DC / MPPT
pub const REG_DC_VOLTAGE_V:        u16 = 27;  // float32  V
pub const REG_DC_CURRENT_A:        u16 = 29;  // float32  A
pub const REG_DC_POWER_KW:         u16 = 31;  // float32  kW
pub const REG_MPPT_VOLTAGE_V:      u16 = 33;  // float32  V
pub const REG_MPPT_CURRENT_A:      u16 = 35;  // float32  A

/// Thermal
pub const REG_INVERTER_TEMP_C:     u16 = 37;  // float32  °C  (heatsink)
pub const REG_AMBIENT_TEMP_C:      u16 = 39;  // float32  °C

/// Performance & Irradiance
pub const REG_EFFICIENCY_PCT:      u16 = 41;  // float32  %
pub const REG_POA_IRRADIANCE:      u16 = 43;  // float32  W/m²
pub const REG_SOLAR_ELEVATION:     u16 = 45;  // float32  °
pub const REG_PERF_RATIO:          u16 = 47;  // float32  0-1  (IEC 61724)
pub const REG_SPECIFIC_YIELD:      u16 = 49;  // float32  kWh/kWp
pub const REG_CAPACITY_FACTOR:     u16 = 51;  // float32  %

/// Safety & Alarms
pub const REG_ISOLATION_MOHM:      u16 = 53;  // float32  MΩ
pub const REG_FAULT_CODE:          u16 = 55;  // u16      IEC fault code
pub const REG_ALARM_FLAGS:         u16 = 56;  // u16      bitmask (alarm_flag_bits)

/// Energy Counters
pub const REG_DAILY_ENERGY_KWH:    u16 = 57;  // float32  kWh
pub const REG_MONTHLY_ENERGY_KWH:  u16 = 59;  // float32  kWh
pub const REG_TOTAL_ENERGY_KWH:    u16 = 61;  // float32  kWh

/// Total registers per plant: 63 (offsets 0..=62).

// ─── Variable type enum ───────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub enum VariableType {
    // ── float32 (2 registers) ──
    PowerKw,
    VoltageL1V, VoltageL2V, VoltageL3V,
    CurrentL1A, CurrentL2A, CurrentL3A,
    FrequencyHz, RocofHzS,
    TemperatureC, InverterTempC, AmbientTempC,
    DcVoltageV, DcCurrentA, DcPowerKw,
    MpptVoltageV, MpptCurrentA,
    ReactivePowerKvar, ApparentPowerKva, PowerFactor,
    EfficiencyPct, PoaIrradianceWM2, SolarElevationDeg,
    PerformanceRatio, SpecificYieldKwhKwp, CapacityFactorPct,
    IsolationMohm,
    DailyEnergyKwh, MonthlyEnergyKwh, TotalEnergyKwh,
    // ── u16 raw (1 register) ──
    Status,
    FaultCode,
    AlarmFlags,
}

/// Encode a f32 into two big-endian u16 words (IEEE 754).
fn float_to_words(v: f32) -> (u16, u16) {
    let bits = v.to_bits();
    ((bits >> 16) as u16, (bits & 0xFFFF) as u16)
}

struct MbService {
    state: AppState,
    register_map: HashMap<u16, (String, VariableType, u8)>,
}

impl Service for MbService {
    type Request = Request<'static>;
    type Response = Response;
    type Exception = ExceptionCode;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Exception>> + Send + Sync>>;

    fn call(&self, req: Self::Request) -> Self::Future {
        let state = self.state.clone();
        let register_map = self.register_map.clone();

        Box::pin(async move {
            let resolve = |reg_addr: u16| -> u16 {
                let Some((plant_id, var_type, word_idx)) = register_map.get(&reg_addr) else { return 0 };
                let Some(data)                           = state.get_data(plant_id)     else { return 0 };

                match var_type {
                    // ── u16 single-register variables ──────────────────────
                    VariableType::Status     => data.status,
                    VariableType::FaultCode  => data.fault_code,
                    VariableType::AlarmFlags => data.alarm_flags as u16,

                    // ── float32 two-register variables ─────────────────────
                    _ => {
                        let f: f32 = match var_type {
                            VariableType::PowerKw              => data.power_kw               as f32,
                            VariableType::VoltageL1V           => data.voltage_l1_v           as f32,
                            VariableType::VoltageL2V           => data.voltage_l2_v           as f32,
                            VariableType::VoltageL3V           => data.voltage_l3_v           as f32,
                            VariableType::CurrentL1A           => data.current_l1_a           as f32,
                            VariableType::CurrentL2A           => data.current_l2_a           as f32,
                            VariableType::CurrentL3A           => data.current_l3_a           as f32,
                            VariableType::FrequencyHz          => data.frequency_hz           as f32,
                            VariableType::RocofHzS             => data.rocof_hz_s             as f32,
                            VariableType::TemperatureC         => data.temperature_c          as f32,
                            VariableType::InverterTempC        => data.inverter_temp_c        as f32,
                            VariableType::AmbientTempC         => data.ambient_temp_c         as f32,
                            VariableType::DcVoltageV           => data.dc_voltage_v           as f32,
                            VariableType::DcCurrentA           => data.dc_current_a           as f32,
                            VariableType::DcPowerKw            => data.dc_power_kw            as f32,
                            VariableType::MpptVoltageV         => data.mppt_voltage_v         as f32,
                            VariableType::MpptCurrentA         => data.mppt_current_a         as f32,
                            VariableType::ReactivePowerKvar    => data.reactive_power_kvar    as f32,
                            VariableType::ApparentPowerKva     => data.apparent_power_kva     as f32,
                            VariableType::PowerFactor          => data.power_factor           as f32,
                            VariableType::EfficiencyPct        => data.efficiency_percent     as f32,
                            VariableType::PoaIrradianceWM2     => data.poa_irradiance_w_m2    as f32,
                            VariableType::SolarElevationDeg    => data.solar_elevation_deg    as f32,
                            VariableType::PerformanceRatio     => data.performance_ratio      as f32,
                            VariableType::SpecificYieldKwhKwp  => data.specific_yield_kwh_kwp as f32,
                            VariableType::CapacityFactorPct    => data.capacity_factor_percent as f32,
                            VariableType::IsolationMohm        => data.isolation_resistance_mohm as f32,
                            VariableType::DailyEnergyKwh       => data.daily_energy_kwh       as f32,
                            VariableType::MonthlyEnergyKwh     => data.monthly_energy_kwh     as f32,
                            VariableType::TotalEnergyKwh       => data.total_energy_kwh       as f32,
                            // u16 variants handled above — unreachable here
                            VariableType::Status | VariableType::FaultCode | VariableType::AlarmFlags => 0.0,
                        };
                        let (high, low) = float_to_words(f);
                        if *word_idx == 0 { high } else { low }
                    }
                }
            };

            match req {
                Request::ReadInputRegisters(addr, cnt) => {
                    let regs: Vec<u16> = (0..cnt).map(|i| resolve(addr + i)).collect();
                    Ok(Response::ReadInputRegisters(regs))
                }
                Request::ReadHoldingRegisters(addr, cnt) => {
                    let regs: Vec<u16> = (0..cnt).map(|i| resolve(addr + i)).collect();
                    Ok(Response::ReadHoldingRegisters(regs))
                }
                _ => Err(ExceptionCode::IllegalFunction),
            }
        })
    }
}

pub async fn run_server(
    addr: SocketAddr,
    state: AppState,
    register_map: HashMap<u16, (String, VariableType, u8)>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Modbus TCP server listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let server = tokio_modbus::server::tcp::Server::new(listener);

    let on_connected = move |socket, _addr| {
        let state        = state.clone();
        let register_map = register_map.clone();
        async move { Ok::<_, std::io::Error>(Some((MbService { state, register_map }, socket))) }
    };

    server.serve(&on_connected, |err| { eprintln!("Modbus server error: {:?}", err); }).await?;
    Ok(())
}
