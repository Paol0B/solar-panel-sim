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
                                match var_type {
                                    VariableType::Power => (data.power_kw * 100.0) as u16,
                                    VariableType::Voltage => (data.voltage_v * 10.0) as u16,
                                    VariableType::Current => (data.current_a * 100.0) as u16,
                                    VariableType::Frequency => (data.frequency_hz * 100.0) as u16,
                                    VariableType::Temperature => (data.temperature_c * 10.0) as u16,
                                    VariableType::Status => data.status,
                                }
                            } else {
                                0
                            }
                        } else {
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
                                match var_type {
                                    VariableType::Power => (data.power_kw * 100.0) as u16,
                                    VariableType::Voltage => (data.voltage_v * 10.0) as u16,
                                    VariableType::Current => (data.current_a * 100.0) as u16,
                                    VariableType::Frequency => (data.frequency_hz * 100.0) as u16,
                                    VariableType::Temperature => (data.temperature_c * 10.0) as u16,
                                    VariableType::Status => data.status,
                                }
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
