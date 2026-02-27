# Configurazione Modbus TCP

## Connessione

- **Host**: `localhost` (o indirizzo IP)
- **Port**: `5020`
- **Protocol**: Modbus TCP (non RTU)
- **Slave ID / Unit ID**: **Ignorato** — il server risponde a qualsiasi slave ID (da 0 a 255)
- **Registers Type**: sia **Input Registers** (0x03) che **Holding Registers** (0x04) sono supportati

## Schema dei Registri

Ogni impianto occupa **63 registri consecutivi** con parametri di configurazione:

```
Plant 1:   base = 0     → registri 0–62
Plant 2:   base = 200   → registri 200–262
Plant 3:   base = 400   → registri 400–462
```

## Tipi di Dato

### Float32 (IEEE 754 big-endian)
Occupa **2 registri consecutivi**:
- **Registro N** = parte alta (high word) 
- **Registro N+1** = parte bassa (low word)

**Esempio**: `power_kw` è al registro offset 0 per plant_1:
- Leggere registri `0–1` → decodificare come float32 IEEE 754

### u16 (Integer)
Occupa **1 registro**

**Esempio**: `status` è al registro offset 10 per plant_1:
- Leggere registro `10` → valore diretto

## Mappa Registri Completa

| Offset | Nome | Tipo | Unità |
|---|---|---|---|
| 0 | `power_kw` | f32 | kW (AC output) |
| 2 | `voltage_l1_v` | f32 | V |
| 4 | `current_l1_a` | f32 | A |
| 6 | `frequency_hz` | f32 | Hz |
| 8 | `temperature_c` | f32 | °C (cell) |
| **10** | **`status`** | **u16** | enum (0=Stop, 1=Run, 2=Fault, 3=Curtail, 4=Start, 5=MPPT) |
| 11 | `voltage_l2_v` | f32 | V |
| 13 | `voltage_l3_v` | f32 | V |
| 15 | `current_l2_a` | f32 | A |
| 17 | `current_l3_a` | f32 | A |
| 19 | `reactive_power_kvar` | f32 | kVar |
| 21 | `apparent_power_kva` | f32 | kVA |
| 23 | `power_factor` | f32 | — |
| 25 | `rocof_hz_s` | f32 | Hz/s |
| 27 | `dc_voltage_v` | f32 | V |
| 29 | `dc_current_a` | f32 | A |
| 31 | `dc_power_kw` | f32 | kW (DC input) |
| 33 | `mppt_voltage_v` | f32 | V |
| 35 | `mppt_current_a` | f32 | A |
| 37 | `inverter_temp_c` | f32 | °C (heatsink) |
| 39 | `ambient_temp_c` | f32 | °C |
| 41 | `efficiency_pct` | f32 | % |
| 43 | `poa_irradiance_w_m2` | f32 | W/m² |
| 45 | `solar_elevation_deg` | f32 | ° |
| 47 | `performance_ratio` | f32 | 0–1 |
| 49 | `specific_yield_kwh_kwp` | f32 | kWh/kWp |
| 51 | `capacity_factor_pct` | f32 | % |
| 53 | `isolation_mohm` | f32 | MΩ |
| **55** | **`fault_code`** | **u16** | IEC code |
| **56** | **`alarm_flags`** | **u16** | bitmask |
| 57 | `daily_energy_kwh` | f32 | kWh |
| 59 | `monthly_energy_kwh` | f32 | kWh |
| 61 | `total_energy_kwh` | f32 | kWh |

## Decodifica F32 (IEEE 754 big-endian)

**Esempio per `power_kw` = 2000 kW**:

```
Registri letti: [0x4448] [0x0000]   (lettura 0–2 registri)

Combinare in 32-bit:  0x44480000
Decodificare come IEEE 754 float:  2000.0 kW ✓
```

### ⚠️ Errore Comune: Little Endian

Se leggi come **little-endian** otterrai numeri **completamente sbagliati**:
```
[0x4448] [0x0000]  →  little-endian  →  0x00004448  →  17480 (SBAGLIATO!)
```

**Soluzione**: Assicurati che il tuo client legga in **big-endian (big-endian byte order)**.

## Configurazione in strumenti comuni

### Modbus Poll (Schneider Electric)
- Protocol: TCP
- Host: localhost:5020
- Slave ID: 1 (cualsiasi valore ok)
- Registers: **Input** or **Holding**
- Data Type: **Float (Big Endian)** per f32 a 2 registri

### QModMaster
- Mode: TCP
- IP: localhost, Port: 5020
- Slave ID: 1
- Byte Order: **Big Endian**

### Python pymodbus
```python
from pymodbus.client import ModbusTcpClient
import struct

client = ModbusTcpClient('localhost', port=5020)
client.connect()

# Leggere power_kw (registri 0–1)
regs = client.read_input_registers(address=0, count=2)
high, low = regs.registers[0], regs.registers[1]

# Decodificare IEEE 754 big-endian
raw = (high << 16) | low
power_kw = struct.unpack('>f', struct.pack('>I', raw))[0]
print(f"Power: {power_kw} kW")

client.close()
```

## Verifica Connessione

```bash
# Test con nc (netcat)
nc -zv localhost 5020

# Oppure con telnet
telnet localhost 5020
```

## Nota Importante: Slave ID

Il server Modbus accetta **qualsiasi slave ID** (0–255). Non devi configurarne uno specifico:
- Se usi slave ID = 1 → OK
- Se usi slave ID = 47 → OK
- Il server risponde sempre

**Questo è configurabile nel codice** in caso tu voglia limitare a uno specifico slave ID.

---

**Vedi questo file per eventuali problemi comuni**:
- Byte order sbagliato? → Usa big-endian (not little-endian)
- Numeri giganteschi (>10^8)? → Probabilmente stai leggendo registri singoli come f32 (che ha 2 registri)
- Slave ID non risponde? → Non è un problema, il server ignora il campo
