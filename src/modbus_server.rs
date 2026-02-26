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

/// Encode a raw f32 value into two u16 big-endian words (IEEE 754).
/// high = bits 31..16, low = bits 15..0
fn float_to_words(v: f32) -> (u16, u16) {
    let bits = v.to_bits();
    ((bits >> 16) as u16, (bits & 0xFFFF) as u16)
}

struct MbService {
    state: AppState,
    /// Map from register address to (plant_id, variable_type, word_index)
    /// word_index: 0 = high word (bits 31..16), 1 = low word (bits 15..0)
    /// Status uses only word_index=0 and is stored as raw u16.
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
            // Shared helper: resolve a register address to a u16 value.
            let resolve = |reg_addr: u16| -> u16 {
                if let Some((plant_id, var_type, word_idx)) = register_map.get(&reg_addr) {
                    if let Some(data) = state.get_data(plant_id) {
                        let val = match var_type {
                            VariableType::Status => {
                                // Status: single u16, word_idx always 0
                                data.status
                            }
                            _ => {
                                // All numeric variables: IEEE 754 float32 split into 2 x u16
                                let f = match var_type {
                                    VariableType::Power       => data.power_kw as f32,
                                    VariableType::Voltage     => data.voltage_v as f32,
                                    VariableType::Current     => data.current_a as f32,
                                    VariableType::Frequency   => data.frequency_hz as f32,
                                    VariableType::Temperature => data.temperature_c as f32,
                                    VariableType::Status      => unreachable!(),
                                };
                                let (high, low) = float_to_words(f);
                                println!(
                                    "[MODBUS] Plant:{} {:?} = {:.4} → IEEE754 hi=0x{:04X} lo=0x{:04X} (addr {})",
                                    plant_id, var_type, f, high, low, reg_addr
                                );
                                if *word_idx == 0 { high } else { low }
                            }
                        };
                        return val;
                    }
                }
                0
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

pub async fn run_server(addr: SocketAddr, state: AppState, register_map: HashMap<u16, (String, VariableType, u8)>) -> Result<(), Box<dyn std::error::Error>> {
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
