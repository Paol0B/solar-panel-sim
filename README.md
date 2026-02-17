# ☀️ Solar Panel Simulator

[![Rust](https://img.shields.io/badge/rust-2024-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Modbus TCP](https://img.shields.io/badge/Modbus-TCP-green.svg)](https://en.wikipedia.org/wiki/Modbus)

A real-time solar power plant monitoring and simulation system built with Rust. This simulator fetches real weather data and calculates realistic solar power output for multiple geographic locations, providing both REST API and Modbus TCP interfaces for industrial integration.

## 🌟 Features

- **Real-Time Weather Integration**: Fetches live weather data (solar radiation, temperature) from the Open-Meteo API
- **Realistic Power Calculations**: Accurately simulates solar panel output based on:
  - Shortwave radiation levels
  - Ambient temperature and cell temperature modeling
  - Panel efficiency curves
  - Load factor and temperature coefficients
- **Multi-Plant Support**: Monitor multiple solar installations across different geographic locations
- **Dual Interface**:
  - **REST API**: Full-featured HTTP API with JSON responses
  - **Modbus TCP**: Industrial protocol support for SCADA systems
- **Interactive API Documentation**: Built-in Scalar UI for exploring and testing endpoints
- **Background Simulation**: Continuous data updates every 5 seconds per plant
- **Comprehensive Metrics**: Tracks power, voltage, current, frequency, efficiency, energy production, and more

## 📋 Table of Contents

- [Architecture](#-architecture)
- [Prerequisites](#-prerequisites)
- [Installation](#-installation)
- [Configuration](#-configuration)
- [Usage](#-usage)
- [API Reference](#-api-reference)
- [Modbus TCP Integration](#-modbus-tcp-integration)
- [Development](#-development)
- [Contributing](#-contributing)
- [License](#-license)

## 🏗️ Architecture

### System Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    Solar Panel Simulator                     │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌──────────────┐      ┌──────────────┐      ┌───────────┐  │
│  │   Weather    │──────▶   Power      │──────▶  Shared   │  │
│  │   Service    │      │  Calculator  │      │   State   │  │
│  │ (Open-Meteo) │      │              │      │ (Thread-  │  │
│  └──────────────┘      └──────────────┘      │   Safe)   │  │
│                                               └─────┬─────┘  │
│                                                     │        │
│  ┌──────────────────────────────────────────────────┼─────┐  │
│  │                                                  ▼     │  │
│  │              ┌─────────────────┬─────────────────────┐│  │
│  │              │   REST API      │   Modbus TCP        ││  │
│  │              │   (Port 3000)   │   (Port 5020)       ││  │
│  │              └─────────────────┴─────────────────────┘│  │
│  └──────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
           │                                │
           ▼                                ▼
    Web Clients/Apps                  SCADA Systems
```

### Technology Stack

- **Language**: Rust (Edition 2024)
- **Web Framework**: Axum (async HTTP server)
- **Async Runtime**: Tokio
- **Protocols**: REST API, Modbus TCP
- **Documentation**: OpenAPI 3.0 with Scalar UI
- **Weather Data**: Open-Meteo API

### Key Components

| Component | Purpose |
|-----------|---------|
| **Power Service** | Fetches weather data and calculates solar power output |
| **Shared State** | Thread-safe storage for real-time plant metrics |
| **API Controllers** | HTTP request handlers for REST endpoints |
| **Modbus Server** | Industrial protocol server for SCADA integration |
| **Background Workers** | Continuous simulation tasks (one per plant) |

## 📦 Prerequisites

- **Rust**: Version 1.70 or higher
- **Cargo**: Comes with Rust installation
- **Internet Connection**: Required for real-time weather data (optional for simulation mode)

### Installation

Install Rust using [rustup](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## 🚀 Installation

1. **Clone the repository**

```bash
git clone https://github.com/Paol0B/solar-panel-sim.git
cd solar-panel-sim
```

2. **Build the project**

```bash
cargo build --release
```

3. **Run the simulator**

```bash
cargo run --release
```

The application will start with:
- REST API server on `http://localhost:3000`
- Modbus TCP server on `localhost:5020`
- Interactive API documentation at `http://localhost:3000/scalar`

## ⚙️ Configuration

The simulator is configured via the `config.json` file in the root directory.

### Configuration Structure

```json
{
  "server": {
    "port": 3000
  },
  "modbus": {
    "port": 5020
  },
  "plants": [
    {
      "id": "plant_1",
      "name": "Turin Main Plant",
      "latitude": 45.07,
      "longitude": 7.33,
      "nominal_power_kw": 1000.0,
      "timezone": "Europe/Rome",
      "modbus_mapping": {
        "power_address": 0,
        "voltage_address": 1,
        "current_address": 2,
        "frequency_address": 3,
        "temperature_address": 4,
        "status_address": 5
      }
    }
  ]
}
```

### Configuration Parameters

#### Server Settings

| Parameter | Type | Description | Default |
|-----------|------|-------------|---------|
| `server.port` | number | HTTP server port | 3000 |
| `modbus.port` | number | Modbus TCP server port | 5020 |

#### Plant Configuration

Each plant in the `plants` array supports the following parameters:

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `id` | string | ✅ | Unique identifier for the plant |
| `name` | string | ✅ | Human-readable plant name |
| `latitude` | number | ✅ | Geographic latitude (-90 to 90) |
| `longitude` | number | ✅ | Geographic longitude (-180 to 180) |
| `nominal_power_kw` | number | ✅ | Nominal power capacity in kilowatts |
| `timezone` | string | ✅ | IANA timezone identifier (e.g., "Europe/Rome") |
| `modbus_mapping` | object | ✅ | Modbus register address mappings |

#### Modbus Mapping

Each plant requires Modbus register addresses for the following metrics:

| Register | Data Type | Unit | Description |
|----------|-----------|------|-------------|
| `power_address` | Float32 | kW | Current power output |
| `voltage_address` | Float32 | V | AC voltage |
| `current_address` | Float32 | A | AC current |
| `frequency_address` | Float32 | Hz | AC frequency |
| `temperature_address` | Float32 | °C | Panel temperature |
| `status_address` | UInt16 | - | Plant status (0=stopped, 1=running) |

### Example Configurations

#### Small Residential Installation

```json
{
  "id": "home_rooftop",
  "name": "Home Rooftop System",
  "latitude": 40.7128,
  "longitude": -74.0060,
  "nominal_power_kw": 10.0,
  "timezone": "America/New_York",
  "modbus_mapping": {
    "power_address": 0,
    "voltage_address": 1,
    "current_address": 2,
    "frequency_address": 3,
    "temperature_address": 4,
    "status_address": 5
  }
}
```

#### Large Commercial Plant

```json
{
  "id": "commercial_farm",
  "name": "Desert Solar Farm",
  "latitude": 36.778259,
  "longitude": -119.417931,
  "nominal_power_kw": 50000.0,
  "timezone": "America/Los_Angeles",
  "modbus_mapping": {
    "power_address": 100,
    "voltage_address": 101,
    "current_address": 102,
    "frequency_address": 103,
    "temperature_address": 104,
    "status_address": 105
  }
}
```

## 💻 Usage

### Starting the Simulator

```bash
# Development mode with hot reloading
cargo run

# Production mode (optimized)
cargo run --release

# Custom configuration file
CONFIG_PATH=./my-config.json cargo run --release
```

### Accessing the API

Once running, you can access:

- **API Base URL**: `http://localhost:3000/api`
- **Interactive Documentation**: `http://localhost:3000/scalar`
- **Static Files**: `http://localhost:3000/static`

### Quick Examples

#### Get All Plants

```bash
curl http://localhost:3000/api/plants
```

Response:
```json
[
  {
    "id": "plant_1",
    "name": "Turin Main Plant",
    "latitude": 45.07,
    "longitude": 7.33,
    "nominal_power_kw": 1000.0
  }
]
```

#### Get Plant Power Data

```bash
curl http://localhost:3000/api/plants/plant_1/power
```

Response:
```json
{
  "plant_id": "plant_1",
  "plant_name": "Turin Main Plant",
  "timestamp": "2026-02-17T16:30:00Z",
  "power_kw": 650.5,
  "voltage_v": 400.2,
  "current_a": 1626.7,
  "frequency_hz": 50.0,
  "temperature_c": 35.2,
  "efficiency_percent": 85.3,
  "status": "running",
  "daily_energy_kwh": 4523.8,
  "power_factor": 0.95,
  "is_day": true,
  "weather_code": 0
}
```

#### Get Global Power Metrics

```bash
curl http://localhost:3000/api/power/global
```

Response:
```json
{
  "total_power_kw": 15234.7,
  "total_plants": 3,
  "timestamp": "2026-02-17T16:30:00Z",
  "plants": [...]
}
```

## 📖 API Reference

### Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/plants` | List all configured plants |
| GET | `/api/plants/{id}/power` | Get real-time power data for a specific plant |
| GET | `/api/power/global` | Get aggregated power data for all plants |
| GET | `/api/modbus/info` | Get Modbus register mapping information |
| GET | `/scalar` | Interactive API documentation |
| GET | `/static/*` | Static file server |

### Response Models

#### PlantInfo

```json
{
  "id": "string",
  "name": "string",
  "latitude": "number",
  "longitude": "number",
  "nominal_power_kw": "number"
}
```

#### PowerPoint

```json
{
  "plant_id": "string",
  "plant_name": "string",
  "timestamp": "ISO8601 datetime",
  "power_kw": "number",
  "voltage_v": "number",
  "current_a": "number",
  "frequency_hz": "number",
  "temperature_c": "number",
  "efficiency_percent": "number",
  "status": "running | stopped",
  "daily_energy_kwh": "number",
  "power_factor": "number",
  "reactive_power_kvar": "number",
  "apparent_power_kva": "number",
  "is_day": "boolean",
  "weather_code": "number"
}
```

#### GlobalPower

```json
{
  "total_power_kw": "number",
  "total_plants": "number",
  "timestamp": "ISO8601 datetime",
  "plants": ["PowerPoint array"]
}
```

### Error Responses

All endpoints return appropriate HTTP status codes:

- `200 OK`: Request successful
- `404 Not Found`: Plant ID not found
- `500 Internal Server Error`: Server error

Error format:
```json
{
  "error": "Error description"
}
```

## 🔌 Modbus TCP Integration

### Overview

The simulator includes a Modbus TCP server for integration with industrial SCADA systems, PLCs, and monitoring software.

### Connection Details

- **Protocol**: Modbus TCP
- **Default Port**: 5020
- **Slave ID**: 1
- **Byte Order**: Big-endian (network order)

### Register Layout

Each plant has its data mapped to Modbus holding registers as defined in `config.json`. Registers store 32-bit floating-point values (occupying 2 consecutive registers each) or 16-bit unsigned integers.

#### Data Types

| Metric | Modbus Type | Registers | Unit |
|--------|-------------|-----------|------|
| Power | Float32 | 2 | kW |
| Voltage | Float32 | 2 | V |
| Current | Float32 | 2 | A |
| Frequency | Float32 | 2 | Hz |
| Temperature | Float32 | 2 | °C |
| Status | UInt16 | 1 | 0 or 1 |

#### Example Register Map (Plant 1)

Based on default configuration:

```
Address  | Metric           | Type    | Value Example
---------|------------------|---------|---------------
0-1      | Power (kW)       | Float32 | 650.5
2-3      | Voltage (V)      | Float32 | 400.2
4-5      | Current (A)      | Float32 | 1626.7
6-7      | Frequency (Hz)   | Float32 | 50.0
8-9      | Temperature (°C) | Float32 | 35.2
10       | Status           | UInt16  | 1 (running)
```

### Connecting with Modbus Clients

#### Python Example (pymodbus)

```python
from pymodbus.client import ModbusTcpClient

# Connect to simulator
client = ModbusTcpClient('localhost', port=5020)
client.connect()

# Read power (registers 0-1)
result = client.read_holding_registers(address=0, count=2, slave=1)
power_kw = struct.unpack('>f', struct.pack('>HH', *result.registers))[0]
print(f"Power: {power_kw} kW")

client.close()
```

#### Node-RED Example

```json
[
  {
    "id": "modbus-read",
    "type": "modbus-read",
    "name": "Read Solar Power",
    "topic": "",
    "showStatusActivities": true,
    "logIOActivities": false,
    "showErrors": true,
    "unitid": "1",
    "dataType": "HoldingRegister",
    "adr": "0",
    "quantity": "2",
    "rate": "5000",
    "server": "modbus-server"
  }
]
```

### Getting Register Information

Query the REST API for Modbus configuration:

```bash
curl http://localhost:3000/api/modbus/info
```

## 🛠️ Development

### Project Structure

```
solar-panel-sim/
├── src/
│   ├── main.rs                 # Application entry point
│   ├── config.rs               # Configuration management
│   ├── shared_state.rs         # Thread-safe state management
│   ├── api_docs.rs             # OpenAPI documentation
│   ├── modbus_server.rs        # Modbus TCP server
│   ├── models/
│   │   ├── mod.rs
│   │   └── power.rs            # Data models
│   ├── services/
│   │   ├── mod.rs
│   │   └── power_service.rs    # Weather & power calculation
│   ├── controllers/
│   │   ├── mod.rs
│   │   └── power_controller.rs # API handlers
│   └── routes/
│       ├── mod.rs
│       └── power_routes.rs     # Route definitions
├── static/
│   └── js/
│       └── app.js              # Frontend JavaScript
├── Cargo.toml                  # Dependencies
├── config.json                 # Configuration file
└── README.md                   # This file
```

### Building from Source

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Run tests
cargo test

# Check for errors without building
cargo check

# Format code
cargo fmt

# Run linter
cargo clippy
```

### Dependencies

Key dependencies from `Cargo.toml`:

```toml
[dependencies]
tokio = { version = "1.48.0", features = ["full"] }
axum = "0.8.8"
reqwest = { version = "0.12.28", features = ["json"] }
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0"
chrono = { version = "0.4", features = ["serde"] }
utoipa = { version = "5.4.0", features = ["axum_extras", "chrono"] }
utoipa-scalar = { version = "0.3.0" }
tokio-modbus = { version = "0.17.0", features = ["tcp-server"] }
tower-http = { version = "0.6.8", features = ["fs", "trace"] }
```

### Development Workflow

1. **Make Changes**: Edit source files in `src/`
2. **Check Code**: Run `cargo check` for quick error checking
3. **Format**: Run `cargo fmt` to format code
4. **Lint**: Run `cargo clippy` to catch common mistakes
5. **Test**: Run `cargo test` (when tests are available)
6. **Run**: Test with `cargo run`

### Debugging

Enable detailed logging by setting the `RUST_LOG` environment variable:

```bash
# Info level
RUST_LOG=info cargo run

# Debug level
RUST_LOG=debug cargo run

# Trace level (very verbose)
RUST_LOG=trace cargo run
```

## 🧮 Power Calculation Model

The simulator uses a realistic photovoltaic model:

### Solar Power Equation

```
P = P_nom × (G/1000) × [1 + α(T_cell - 25)]
```

Where:
- `P_nom` = Nominal power capacity (kW)
- `G` = Solar irradiance (W/m²)
- `T_cell` = Cell temperature (°C)
- `α` = Temperature coefficient (-0.004/°C)

### Cell Temperature Model

```
T_cell = T_ambient + (NOCT - 20) × (G/800)
```

Where:
- `NOCT` = Nominal Operating Cell Temperature (45°C)
- `T_ambient` = Ambient temperature (°C)

### Efficiency Calculation

```
η = η_nom × LoadFactor × [1 + α(T_cell - 25)]
```

### AC Output

The simulator also calculates AC characteristics:
- **Voltage**: Nominal 400V ± 2% variation
- **Current**: Calculated from power and voltage
- **Frequency**: 50 Hz ± 0.1 Hz variation
- **Power Factor**: 0.95 typical
- **Reactive Power**: Calculated from real power and power factor
- **Apparent Power**: Calculated from real and reactive power

## 🤝 Contributing

Contributions are welcome! Here's how you can help:

1. **Fork the repository**
2. **Create a feature branch**: `git checkout -b feature/amazing-feature`
3. **Make your changes**: Follow Rust best practices and formatting
4. **Test your changes**: Ensure everything works correctly
5. **Commit your changes**: `git commit -m 'Add amazing feature'`
6. **Push to the branch**: `git push origin feature/amazing-feature`
7. **Open a Pull Request**

### Code Style

- Follow Rust standard formatting (`cargo fmt`)
- Fix all `cargo clippy` warnings
- Write clear commit messages
- Document public APIs with doc comments

### Reporting Issues

Found a bug or have a feature request? Please open an issue on GitHub with:
- Clear description of the problem or feature
- Steps to reproduce (for bugs)
- Expected vs actual behavior
- Your environment (OS, Rust version, etc.)

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- **Open-Meteo API**: For providing free weather data
- **Rust Community**: For excellent async ecosystem (Tokio, Axum)
- **Modbus Community**: For industrial protocol support

## 📧 Contact

For questions or support, please open an issue on GitHub.

---

**Made with ☀️ and 🦀 Rust**
