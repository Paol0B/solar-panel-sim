# Modbus Scaling Update - Feb 26, 2026

## Problema Identificato

L'impianto California (50 MW nominali) produceva ~5000 kW ma i valori Modbus mostravano valori vicini allo zero o negativi (-1.00, -0.00).

### Cause Root

1. **Overflow u16**: Con scala ×100, il valore massimo rappresentabile era solo 655.35 kW
   - 5000 kW × 100 = 500,000 
   - u16::MAX = 65,535
   - Risultato: overflow e valori troncati/errati

2. **Problemi di calcolo corrente**: La corrente veniva calcolata prima che il power_factor fosse settato correttamente, specialmente per potenze basse o zero

3. **Valori negativi per errori di floating point**: Cast da f64 negativo (anche piccolissimo) a u16 causava wraparound

## Soluzioni Implementate

### 1. Nuovo Scaling Modbus (modbus_server.rs)

**Prima:**
```rust
Power:       (data.power_kw * 100.0) as u16    // Max: 655.35 kW
Current:     (data.current_a * 100.0) as u16   // Max: 655.35 A
```

**Dopo:**
```rust
Power:       data.power_kw (integer)           // Max: 65,535 kW (~65 MW)
Voltage:     data.voltage_v * 10               // Max: 6,553.5 V
Current:     data.current_a * 10               // Max: 6,553.5 A
Frequency:   data.frequency_hz * 100           // Max: 655.35 Hz
Temperature: data.temperature_c * 10           // Max: 6,553.5 °C
```

### 2. Fix Calcolo Corrente (shared_state.rs)

- Aggiunta protezione per valori sotto soglia (0.01 kW)
- Power factor impostato a 1.0 quando potenza è zero
- Calcolo corrente solo se potenza > 0.01 kW
- Riordino calcoli: power_factor → power_kw → apparent_power → current

### 3. Protezioni Aggiuntive

- `.max(0.0)` prima di casting per evitare valori negativi
- `.round()` per conversione accurata a intero
- `.min(65535)` per clipping esplicito

### 4. Logging Estensivo

Aggiunto logging in 3 punti chiave:
1. **main.rs**: Update dei dati da weather API
2. **shared_state.rs**: Salvataggio nello state condiviso
3. **modbus_server.rs**: Lettura registri Modbus

### 5. Documentazione Aggiornata

- **API Documentation** (controllers/power_controller.rs): Descrizioni registro aggiornate
- **README.md**: Tabelle scaling, esempi Python/Node-RED corretti

## Scaling Reference

| Variable | Old Scale | New Scale | Format | Max Value |
|----------|-----------|-----------|--------|-----------|
| Power | ×100 (centi-kW) | ×1 (kW) | UInt16 | 65.5 MW |
| Voltage | ×10 (deci-V) | ×10 (deci-V) | UInt16 | 6553.5 V |
| Current | ×100 (centi-A) | ×10 (deci-A) | UInt16 | 6553.5 A |
| Frequency | ×100 (centi-Hz) | ×100 (centi-Hz) | UInt16 | 655.35 Hz |
| Temperature | ×10 (deci-°C) | ×10 (deci-°C) | UInt16 | 6553.5 °C |
| Status | (none) | (none) | UInt16 | 0 or 1 |

## Verifica Binding Plant-Modbus

Il binding è verificato corretto tramite:
- Register map costruita iterando su `config.plants` con `plant.id` come chiave
- State updates usano `plant_config.id` dal background task
- Modbus server usa la stessa `plant_id` per recuperare dati

**Esempio Log Flow:**
```
[UPDATE] Plant: plant_3 | DC Power: 5432.15 kW | Temp: 35.2°C
[STATE UPDATE] Plant: plant_3 | AC Power: 5321.45 kW | Current: 23.4 A
[MODBUS READ] Plant: plant_3 | Var: Power | Raw: 5321.45 | Scaled: 5321
```

## Testing

Per verificare il corretto funzionamento:

1. **Check logs all'avvio**:
   ```
   [MODBUS MAP] Plant: plant_3 -> Power@20, Voltage@21, ...
   ```

2. **Monitor updates ogni 5 secondi**:
   ```
   [UPDATE] Plant: plant_3 | DC Power: ... kW
   [STATE UPDATE] Plant: plant_3 | AC Power: ... kW
   ```

3. **Test lettura Modbus**:
   ```bash
   # Python
   python3 -c "from pymodbus.client import ModbusTcpClient; \
   c = ModbusTcpClient('localhost', 5020); c.connect(); \
   r = c.read_holding_registers(20, 6, slave=1); \
   print(f'Power: {r.registers[0]} kW'); c.close()"
   ```

4. **Check API REST**:
   ```bash
   curl http://localhost:3000/api/plants/plant_3/power | jq '.data.power_kw'
   ```

## Compatibility Note

⚠️ **BREAKING CHANGE**: Client applications devono aggiornare il decoding:

**Prima:**
```python
power_kw = register_value / 100.0  # centi-kW
current_a = register_value / 100.0  # centi-A
```

**Dopo:**
```python
power_kw = register_value  # kW diretti
current_a = register_value / 10.0  # deci-A
```

## Files Modified

- `src/modbus_server.rs`: Scaling logic e logging
- `src/shared_state.rs`: Calcolo corrente e power factor
- `src/main.rs`: Logging updates e modbus mapping
- `src/controllers/power_controller.rs`: API documentation
- `README.md`: Documentation e esempi
