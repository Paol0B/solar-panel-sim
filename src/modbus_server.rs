use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use tokio_modbus::prelude::*;
use tokio_modbus::server::Service;
use tokio_modbus::ExceptionCode;

use crate::shared_state::AppState;

#[derive(Clone, Debug)]
pub enum VariableType {
    Power,
    Voltage,
    Current,
    Frequency,
    Temperature,
    Status,
}

struct MbService {
    state: AppState,
    /// Map from register address to (plant_id, variable_type)
    register_map: HashMap<u16, (String, VariableType)>,
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
            match req {
                Request::ReadInputRegisters(addr, cnt) => {
                    let mut registers = Vec::with_capacity(cnt as usize);
                    for i in 0..cnt {
                        let reg_addr = addr + i;
                        let val = if let Some((plant_id, var_type)) = register_map.get(&reg_addr) {
                            if let Some(data) = state.get_data(plant_id) {
                                let scaled_val = match var_type {
                                    // Power: scale x1 (integer kW) - supports up to 65.535 MW
                                    VariableType::Power => (data.power_kw.max(0.0).round() as u32).min(65535),
                                    // Voltage: scale x10 (deci-V) - supports up to 6553.5 V
                                    VariableType::Voltage => ((data.voltage_v * 10.0).max(0.0).round() as u32).min(65535),
                                    // Current: scale x10 (deci-A) - supports up to 6553.5 A
                                    VariableType::Current => ((data.current_a * 10.0).max(0.0).round() as u32).min(65535),
                                    // Frequency: scale x100 (centi-Hz) - supports up to 655.35 Hz
                                    VariableType::Frequency => ((data.frequency_hz * 100.0).max(0.0).round() as u32).min(65535),
                                    // Temperature: scale x10 (deci-C) - supports up to 6553.5 C
                                    VariableType::Temperature => ((data.temperature_c * 10.0).max(0.0).round() as u32).min(65535),
                                    VariableType::Status => data.status as u32,
                                };
                                if i == 0 {
                                    println!("[MODBUS READ] Plant: {} | Var: {:?} | Raw: {:.2} | Scaled: {} | Addr: {}",
                                             plant_id, var_type, 
                                             match var_type {
                                                 VariableType::Power => data.power_kw,
                                                 VariableType::Voltage => data.voltage_v,
                                                 VariableType::Current => data.current_a,
                                                 VariableType::Frequency => data.frequency_hz,
                                                 VariableType::Temperature => data.temperature_c,
                                                 VariableType::Status => data.status as f64,
                                             },
                                             scaled_val, reg_addr);
                                }
                                scaled_val as u16
                            } else {
                                if i == 0 {
                                    println!("[MODBUS READ] Addr: {} | No data for plant", reg_addr);
                                }
                                0
                            }
                        } else {
                            if i == 0 {
                                println!("[MODBUS READ] Addr: {} | Not mapped", reg_addr);
                            }
                            0
                        };
                        registers.push(val);
                    }
                    Ok(Response::ReadInputRegisters(registers))
                }
                Request::ReadHoldingRegisters(addr, cnt) => {
                     let mut registers = Vec::with_capacity(cnt as usize);
                    for i in 0..cnt {
                        let reg_addr = addr + i;
                        let val = if let Some((plant_id, var_type)) = register_map.get(&reg_addr) {
                            if let Some(data) = state.get_data(plant_id) {
                                let scaled_val = match var_type {
                                    // Power: scale x1 (integer kW) - supports up to 65.535 MW
                                    VariableType::Power => (data.power_kw.max(0.0).round() as u32).min(65535),
                                    // Voltage: scale x10 (deci-V) - supports up to 6553.5 V
                                    VariableType::Voltage => ((data.voltage_v * 10.0).max(0.0).round() as u32).min(65535),
                                    // Current: scale x10 (deci-A) - supports up to 6553.5 A
                                    VariableType::Current => ((data.current_a * 10.0).max(0.0).round() as u32).min(65535),
                                    // Frequency: scale x100 (centi-Hz) - supports up to 655.35 Hz
                                    VariableType::Frequency => ((data.frequency_hz * 100.0).max(0.0).round() as u32).min(65535),
                                    // Temperature: scale x10 (deci-C) - supports up to 6553.5 C
                                    VariableType::Temperature => ((data.temperature_c * 10.0).max(0.0).round() as u32).min(65535),
                                    VariableType::Status => data.status as u32,
                                };
                                scaled_val as u16
                            } else {
                                0
                            }
                        } else {
                            0
                        };
                        registers.push(val);
                    }
                    Ok(Response::ReadHoldingRegisters(registers))
                }
                _ => Err(ExceptionCode::IllegalFunction),
            }
        })
    }
}

pub async fn run_server(addr: SocketAddr, state: AppState, register_map: HashMap<u16, (String, VariableType)>) -> Result<(), Box<dyn std::error::Error>> {
    println!("Modbus TCP server listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let server = tokio_modbus::server::tcp::Server::new(listener);
    
    let on_connected = move |socket, _addr| {
        let state = state.clone();
        let register_map = register_map.clone();
        async move { Ok::<_, std::io::Error>(Some((MbService { state, register_map }, socket))) }
    };

    server.serve(&on_connected, |err| { eprintln!("Modbus server error: {:?}", err); }).await?;
    
    Ok(())
}
