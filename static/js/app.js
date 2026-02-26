// ============================================================
//  Solar SCADA — app.js  (UI v3)
// ============================================================

// ---- State ----
let systemConfig       = { api_port: 3000, modbus_port: 5020, modbus_host: '0.0.0.0' };
let plants             = [];
let currentPlantId     = null;
let currentPlantTimezone = 'UTC';
let map                = null;
let markers            = {};
let miniMap            = null;
let powerChart         = null;
let updateInterval     = null;
let modbusInfo         = [];
let chartData          = [];   // { time, kw }[]

// ---- Bootstrap ----
document.addEventListener('DOMContentLoaded', async () => {
    await fetchSystemConfig();  // FIRST: Load configuration
    initMap();
    initChart();
    fetchModbusInfo();
    fetchPlants();
    startClock();
    updatePortDisplays();  // Update displayed ports after config is loaded

    document.getElementById('plant-search').addEventListener('input', filterPlantList);
    document.querySelectorAll('.detail-tab').forEach(btn => {
        btn.addEventListener('click', () => switchTab(btn.dataset.tab));
    });

    setInterval(updateGlobalData, 5000);
});

// ============================================================
//  CLOCK
// ============================================================
function startClock() {
    const el = document.getElementById('clock');
    setInterval(() => { el.innerText = new Date().toLocaleTimeString(); }, 1000);
}

// ============================================================
//  MAIN MAP
// ============================================================
function initMap() {
    map = L.map('map').setView([45.0, 10.0], 5);
    L.tileLayer('https://{s}.basemaps.cartocdn.com/dark_all/{z}/{x}/{y}{r}.png', {
        attribution: '&copy; OpenStreetMap &copy; CARTO',
        subdomains: 'abcd', maxZoom: 19
    }).addTo(map);
}

function renderMapMarkers() {
    Object.values(markers).forEach(m => map.removeLayer(m));
    markers = {};
    plants.forEach(plant => {
        const m = L.marker([plant.latitude, plant.longitude])
            .addTo(map)
            .bindPopup(`<b>${plant.name}</b><br>Nominal: ${plant.nominal_power_kw.toLocaleString()} kW<br>
                <button class="btn btn-sm btn-warning mt-2" onclick="selectPlant('${plant.id}')">View Details</button>`);
        markers[plant.id] = m;
    });
    if (plants.length > 0) {
        const group = new L.featureGroup(Object.values(markers));
        map.fitBounds(group.getBounds().pad(0.15));
    }
}

function showMap() {
    currentPlantId = null;
    if (updateInterval) { clearInterval(updateInterval); updateInterval = null; }
    if (miniMap) { miniMap.remove(); miniMap = null; }
    document.getElementById('map-view').classList.remove('d-none');
    const dv = document.getElementById('detail-view');
    dv.classList.add('d-none');
    dv.classList.remove('d-flex');
    document.querySelectorAll('.plant-item').forEach(el => el.classList.remove('active'));
    setTimeout(() => map.invalidateSize(), 100);
}

// ============================================================
//  CHART
// ============================================================
function initChart() {
    const ctx = document.getElementById('powerChart').getContext('2d');
    powerChart = new Chart(ctx, {
        type: 'line',
        data: {
            labels: [],
            datasets: [{
                label: 'Active Power (kW)',
                data: [],
                borderColor: '#ffc107',
                backgroundColor: 'rgba(255,193,7,0.08)',
                borderWidth: 2,
                fill: true,
                tension: 0.4,
                pointRadius: 2
            }]
        },
        options: {
            responsive: true,
            maintainAspectRatio: false,
            scales: {
                x: { grid: { color: '#1e2336' }, ticks: { color: '#5a6380', font: { size: 10 } } },
                y: { grid: { color: '#1e2336' }, ticks: { color: '#5a6380', font: { size: 10 } }, beginAtZero: true }
            },
            plugins: { legend: { labels: { color: '#8892ab', font: { size: 11 } } } },
            animation: false
        }
    });
}

function updateChart(kw, timeLabel) {
    const MAX_POINTS = 30;
    chartData.push({ time: timeLabel, kw });
    if (chartData.length > MAX_POINTS) chartData.shift();
    if (powerChart) {
        powerChart.data.labels = chartData.map(d => d.time);
        powerChart.data.datasets[0].data = chartData.map(d => d.kw);
        powerChart.update('none');
    }
    const peak = Math.max(...chartData.map(d => d.kw));
    const avg  = chartData.reduce((s, d) => s + d.kw, 0) / chartData.length;
    document.getElementById('chart-peak').innerText    = `${peak.toFixed(2)} kW`;
    document.getElementById('chart-avg').innerText     = `${avg.toFixed(2)} kW`;
    document.getElementById('chart-samples').innerText = chartData.length;
}

// ============================================================
//  SYSTEM CONFIGURATION
// ============================================================
async function fetchSystemConfig() {
    try {
        const res = await fetch('/api/system/config');
        systemConfig = await res.json();
        console.log('[CONFIG] Loaded system configuration:', systemConfig);
    } catch (e) {
        console.error('fetchSystemConfig:', e);
        // Keep defaults if fetch fails
    }
}

function updatePortDisplays() {
    // Update navbar Modbus port display
    const navModbusPort = document.getElementById('nav-modbus-port');
    if (navModbusPort) {
        navModbusPort.textContent = `:${systemConfig.modbus_port}`;
    }
    
    // Update navbar API port display
    const navApiPort = document.getElementById('nav-api-port');
    if (navApiPort) {
        navApiPort.textContent = `:${systemConfig.api_port}`;
    }
    
    // Update Modbus address display in detail view
    const mbAddrEl = document.getElementById('mb-addr');
    if (mbAddrEl) {
        mbAddrEl.textContent = `${systemConfig.modbus_host}:${systemConfig.modbus_port}`;
    }
}

// ============================================================
//  PLANT LIST
// ============================================================
async function fetchPlants() {
    try {
        const res = await fetch('/api/plants');
        plants = await res.json();
        renderPlantList();
        renderMapMarkers();
        updateGlobalData();
    } catch (e) { console.error('fetchPlants:', e); }
}

function renderPlantList() {
    const list = document.getElementById('plant-list');
    list.innerHTML = '';
    plants.forEach(plant => {
        const div = document.createElement('div');
        div.className = 'plant-item';
        div.dataset.id = plant.id;
        div.onclick = () => selectPlant(plant.id);
        div.innerHTML = `
            <div class="d-flex justify-content-between align-items-start">
                <div class="d-flex gap-2 align-items-start">
                    <div class="plant-status-dot mt-1" id="dot-${plant.id}" style="background:#6b7280"></div>
                    <div>
                        <div class="plant-item-name">${plant.name}</div>
                        <div class="plant-item-id">${plant.id}</div>
                    </div>
                </div>
                <div class="text-end">
                    <div class="plant-item-power text-warning" id="list-power-${plant.id}">— kW</div>
                    <div class="plant-item-sub">${plant.nominal_power_kw.toLocaleString()} kW nominal</div>
                </div>
            </div>`;
        list.appendChild(div);
    });
    document.getElementById('sb-plant-count').innerText = plants.length;
    document.getElementById('sb-total').innerText        = plants.length;
    document.getElementById('map-total').innerText       = plants.length;
}

function filterPlantList() {
    const q = document.getElementById('plant-search').value.toLowerCase().trim();
    document.querySelectorAll('.plant-item').forEach(item => {
        const plant = plants.find(p => p.id === item.dataset.id);
        const match = !q || plant.name.toLowerCase().includes(q) || plant.id.toLowerCase().includes(q);
        item.style.display = match ? '' : 'none';
    });
}

// ============================================================
//  GLOBAL DATA POLLING
// ============================================================
async function updateGlobalData() {
    try {
        const res = await fetch('/api/power/global');
        const data = await res.json();
        let totalPower = 0, activeCount = 0;
        for (const [id, power] of Object.entries(data)) {
            totalPower += power;
            if (power > 0.1) activeCount++;
            const el = document.getElementById(`list-power-${id}`);
            if (el) {
                el.innerText    = fmtPower(power);
                el.className    = 'plant-item-power ' + (power > 0.1 ? 'text-success' : 'text-muted');
            }
            const dot = document.getElementById(`dot-${id}`);
            if (dot) dot.style.background = power > 0.1 ? '#22c55e' : '#6b7280';
        }
        document.getElementById('sb-total-power').innerText  = fmtPower(totalPower);
        document.getElementById('sb-active').innerText       = activeCount;
        document.getElementById('map-active').innerText      = activeCount;
        document.getElementById('map-total-power').innerText = fmtPower(totalPower);
        const effEst = activeCount > 0 ? Math.round(activeCount / plants.length * 85) : 0;
        document.getElementById('map-avg-eff').innerText = activeCount > 0 ? `${effEst}%` : '—%';
        document.getElementById('sb-last-update').innerText  = new Date().toLocaleTimeString();
    } catch (e) { console.error('updateGlobalData:', e); }
}

// ============================================================
//  SELECT PLANT → DETAIL VIEW
// ============================================================
function selectPlant(id) {
    currentPlantId = id;
    const plant = plants.find(p => p.id === id);
    if (!plant) return;

    document.getElementById('map-view').classList.add('d-none');
    const dv = document.getElementById('detail-view');
    dv.classList.remove('d-none');
    dv.classList.add('d-flex');

    document.querySelectorAll('.plant-item').forEach(el => el.classList.remove('active'));
    const activeItem = document.querySelector(`.plant-item[data-id="${id}"]`);
    if (activeItem) activeItem.classList.add('active');

    document.getElementById('detail-name').innerText    = plant.name;
    document.getElementById('detail-id').innerText      = plant.id;
    document.getElementById('detail-nominal').innerText = plant.nominal_power_kw.toLocaleString();
    currentPlantTimezone = plant.timezone || 'UTC';

    chartData = [];
    if (powerChart) {
        powerChart.data.labels = [];
        powerChart.data.datasets[0].data = [];
        powerChart.update();
    }

    switchTab('tab-kpi');
    renderModbusRegisters(plant, null);
    renderPlantInfo(plant);

    if (updateInterval) clearInterval(updateInterval);
    fetchPlantDetail();
    updateInterval = setInterval(fetchPlantDetail, 2000);
}

// ============================================================
//  TABS
// ============================================================
function switchTab(tabId) {
    document.querySelectorAll('.detail-tab').forEach(btn => {
        btn.classList.toggle('active', btn.dataset.tab === tabId);
    });
    document.querySelectorAll('.tab-panel').forEach(panel => {
        panel.classList.toggle('d-none', panel.id !== tabId);
    });
}

// ============================================================
//  FETCH PLANT DETAIL (2s poll)
// ============================================================
async function fetchPlantDetail() {
    if (!currentPlantId) return;
    try {
        const res = await fetch(`/api/plants/${currentPlantId}/power`);
        if (!res.ok) return;
        const json = await res.json();
        const d    = json.data;
        const ts   = new Date(json.timestamp);
        const plant = plants.find(p => p.id === currentPlantId);
        const nominal = plant ? plant.nominal_power_kw : 1;

        // Power
        document.getElementById('detail-power').innerText = `${d.power_kw.toFixed(2)} kW`;
        const pct = Math.min(100, (d.power_kw / nominal) * 100);
        document.getElementById('detail-power-bar').style.width = `${pct.toFixed(1)}%`;
        document.getElementById('detail-power-pct').innerText   = `${pct.toFixed(1)}% of nominal`;

        // Voltage & Current
        document.getElementById('detail-voltage').innerText = `${d.voltage_v.toFixed(1)} V`;
        document.getElementById('detail-current').innerText = `${d.current_a.toFixed(2)} A`;

        // Frequency with status
        document.getElementById('detail-frequency').innerText = `${d.frequency_hz.toFixed(2)} Hz`;
        const freqStatus = document.getElementById('detail-freq-status');
        if (d.frequency_hz >= 49.5 && d.frequency_hz <= 50.5) {
            freqStatus.innerHTML = '<span class="text-success small">OK — nominal</span>';
        } else if (d.frequency_hz < 48 || d.frequency_hz > 52) {
            freqStatus.innerHTML = '<span class="text-danger small">OUT OF RANGE</span>';
        } else {
            freqStatus.innerHTML = '<span class="text-warning small">WARNING</span>';
        }

        // Temperature (colour-coded)
        const tempEl = document.getElementById('detail-temp-col');
        tempEl.innerText  = `${d.temperature_c.toFixed(1)} °C`;
        tempEl.className  = 'kpi-card-val ' + (d.temperature_c > 60 ? 'text-danger' : d.temperature_c > 40 ? 'text-warning' : 'text-success');

        // Efficiency
        const eff = d.efficiency_percent || 0;
        document.getElementById('detail-efficiency').innerText    = `${eff.toFixed(1)} %`;
        document.getElementById('detail-eff-bar').style.width     = `${Math.min(100, eff).toFixed(1)}%`;

        // Power Factor
        const pf = d.power_factor || 0;
        document.getElementById('detail-pf').innerText        = pf.toFixed(3);
        document.getElementById('detail-pf-bar').style.width  = `${(Math.abs(pf) * 100).toFixed(1)}%`;

        // Daily Energy
        document.getElementById('detail-energy').innerText = `${d.daily_energy_kwh.toFixed(2)} kWh`;

        // Power Triangle
        document.getElementById('pt-active').innerText   = `${d.power_kw.toFixed(2)} kW`;
        document.getElementById('pt-apparent').innerText = `${d.apparent_power_kva.toFixed(2)} kVA`;
        document.getElementById('pt-reactive').innerText = `${d.reactive_power_kvar.toFixed(2)} kvar`;

        // Local time
        try {
            const local = new Intl.DateTimeFormat('en-GB', {
                timeZone: currentPlantTimezone,
                hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false
            }).format(new Date());
            document.getElementById('detail-local-time').innerText = `${local} (${currentPlantTimezone})`;
        } catch (_) { document.getElementById('detail-local-time').innerText = '--:--'; }

        // Weather icon
        let icon = 'fa-sun', iconCls = 'text-warning';
        if (!d.is_day) { icon = 'fa-moon'; iconCls = 'text-light'; }
        else if (d.weather_code > 3 && d.weather_code <= 50) { icon = 'fa-cloud-sun'; iconCls = 'text-info'; }
        else if (d.weather_code > 50) { icon = 'fa-cloud-rain'; iconCls = 'text-primary'; }
        document.getElementById('weather-icon-container').innerHTML = `<i class="fas ${icon} fa-2x ${iconCls}"></i>`;

        // Status badge & solar array
        const statusEl = document.getElementById('detail-status');
        if (d.status === 1) {
            statusEl.innerText = 'RUNNING'; statusEl.className = 'badge bg-success fs-6';
            document.getElementById('sun-visual').style.opacity = '1';
            document.querySelectorAll('.panel').forEach(p => p.classList.add('active'));
        } else {
            statusEl.innerText = 'STOPPED'; statusEl.className = 'badge bg-secondary fs-6';
            document.getElementById('sun-visual').style.opacity = '0.15';
            document.querySelectorAll('.panel').forEach(p => p.classList.remove('active'));
        }

        // Live Modbus update
        renderModbusRegisters(plant, d);

        // Chart
        updateChart(d.power_kw, ts.toLocaleTimeString());

    } catch (e) { console.error('fetchPlantDetail:', e); }
}

// ============================================================
//  MODBUS TAB
// ============================================================
async function fetchModbusInfo() {
    try {
        const res = await fetch('/api/modbus/info');
        modbusInfo = await res.json();
    } catch (e) { console.error('fetchModbusInfo:', e); }
}

// Encode a JavaScript number as IEEE 754 float32, return the two big-endian u16 words.
function floatToWords(value) {
    const buf = new ArrayBuffer(4);
    new DataView(buf).setFloat32(0, value, false); // big-endian
    const view = new DataView(buf);
    return {
        high: view.getUint16(0, false),
        low:  view.getUint16(2, false),
    };
}

// No scaling factors — values are stored as raw IEEE 754 float32 in 2 consecutive registers.
// Status is the only variable that occupies a single u16 register (raw integer).
const VAR_MAP = {
    'Power':       { key: 'power_kw',       unit: 'kW',  regs: 2 },
    'Voltage':     { key: 'voltage_v',       unit: 'V',   regs: 2 },
    'Current':     { key: 'current_a',       unit: 'A',   regs: 2 },
    'Frequency':   { key: 'frequency_hz',    unit: 'Hz',  regs: 2 },
    'Temperature': { key: 'temperature_c',   unit: '°C',  regs: 2 },
    'Status':      { key: 'status',          unit: '—',   regs: 1 },
};

function detectVar(desc) {
    for (const name of Object.keys(VAR_MAP)) {
        if (desc.toLowerCase().includes(name.toLowerCase())) return name;
    }
    return 'Status';
}

function renderModbusRegisters(plant, liveData) {
    if (!plant) return;

    const dot = document.getElementById('mb-dot');
    dot.className = 'modbus-status-dot ' + (liveData ? 'active' : '');
    document.getElementById('mb-addr').innerText    = '0.0.0.0:5020 · Unit ID 1';
    document.getElementById('mb-plant-id').innerText = plant.id;

    const plantRegs = modbusInfo.filter(r => r.plant_id === plant.id);
    document.getElementById('mb-reg-count').innerText = `${plantRegs.length} registers`;

    const tbody = document.getElementById('mb-register-tbody');
    tbody.innerHTML = '';

    if (plantRegs.length === 0) {
        tbody.innerHTML = '<tr><td colspan="7" class="text-center text-muted py-3 small">No modbus data for this plant</td></tr>';
    }

    plantRegs.forEach(reg => {
        const varName = detectVar(reg.description);
        const varInfo = VAR_MAP[varName];
        let raw = '—', decoded = '—', rowClass = '';

        if (liveData) {
            const rv = liveData[varInfo.key];
            if (rv !== undefined && rv !== null) {
                if (varName === 'Status') {
                    raw      = rv;
                    decoded  = rv === 1 ? '1 (Running)' : '0 (Stopped)';
                    rowClass = rv === 1 ? 'mb-row-ok' : 'mb-row-warn';
                } else {
                    const { high, low } = floatToWords(rv);
                    raw     = `0x${high.toString(16).padStart(4,'0').toUpperCase()} / 0x${low.toString(16).padStart(4,'0').toUpperCase()}`;
                    decoded = rv.toFixed(4);
                    rowClass = 'mb-row-ok';
                }
            }
        }

        const badge = rowClass === 'mb-row-ok'
            ? '<span class="badge bg-success-subtle text-success-emphasis border border-success small">OK</span>'
            : rowClass === 'mb-row-warn'
            ? '<span class="badge bg-warning-subtle text-warning-emphasis border border-warning small">STOPPED</span>'
            : '<span class="badge bg-secondary small text-muted">—</span>';

        // Encoding column: IEEE 754 float32 (2 regs) or u16 raw (1 reg)
        const encLabel = varName === 'Status' ? 'u16 raw (1 reg)' : 'IEEE 754 f32 (2 regs)';

        const tr = document.createElement('tr');
        tr.className = rowClass;
        tr.innerHTML = `
            <td><span class="reg-addr-badge">${reg.register_address}</span></td>
            <td class="text-white">${varName}</td>
            <td class="text-muted font-monospace small">${encLabel}</td>
            <td class="text-end font-monospace text-secondary small">${raw}</td>
            <td class="text-end font-monospace text-info">${decoded}</td>
            <td class="text-center text-muted small">${varInfo.unit}</td>
            <td class="text-center">${badge}</td>`;
        tbody.appendChild(tr);
    });

    // Python snippet
    const baseAddr = plantRegs.length > 0 ? Math.min(...plantRegs.map(r => r.register_address)) : 0;
    document.getElementById('mb-code-snippet').textContent = buildPythonSnippet(plant, plantRegs, baseAddr);
}

function buildPythonSnippet(plant, regs, baseAddr) {
    const lines = [
        `import struct`,
        `from pymodbus.client import ModbusTcpClient`,
        ``,
        `# Plant: ${plant.name} (${plant.id})`,
        `# All numeric values are IEEE 754 float32 packed in 2 consecutive u16 registers`,
        `# (big-endian: high word first). Status is a single u16.`,
        `client = ModbusTcpClient('localhost', port=${systemConfig.modbus_port})`,
        `client.connect()`,
        ``,
        `def read_float32(client, addr):`,
        `    rr = client.read_holding_registers(addr, 2, unit=1)`,
        `    if rr.isError(): return None`,
        `    raw = (rr.registers[0] << 16) | rr.registers[1]`,
        `    return struct.unpack('!f', struct.pack('!I', raw))[0]`,
        ``,
    ];
    regs.forEach(reg => {
        const varName = detectVar(reg.description);
        const vi      = VAR_MAP[varName];
        if (varName === 'Status') {
            lines.push(`# Status`);
            lines.push(`rr = client.read_holding_registers(${reg.register_address}, 1, unit=1)`);
            lines.push(`status = rr.registers[0] if not rr.isError() else None  # 1=Running, 0=Stopped`);
        } else {
            lines.push(`${varName.toLowerCase()} = read_float32(client, ${reg.register_address})  # ${vi.unit}`);
        }
    });
    lines.push(``, `client.close()`);
    return lines.join('\n');
}

// ============================================================
//  PLANT INFO TAB
// ============================================================
function renderPlantInfo(plant) {
    document.getElementById('info-id').innerText      = plant.id;
    document.getElementById('info-name').innerText    = plant.name;
    document.getElementById('info-tz').innerText      = plant.timezone || 'UTC';
    document.getElementById('info-nominal').innerText = `${plant.nominal_power_kw.toLocaleString()} kW`;
    document.getElementById('info-lat').innerText     = plant.latitude.toFixed(6);
    document.getElementById('info-lon').innerText     = plant.longitude.toFixed(6);

    // Mini Leaflet map
    const miniMapEl = document.getElementById('info-mini-map');
    if (miniMap) { miniMap.remove(); miniMap = null; }
    miniMapEl.innerHTML = '';
    setTimeout(() => {
        miniMap = L.map('info-mini-map', { zoomControl: false, attributionControl: false })
            .setView([plant.latitude, plant.longitude], 8);
        L.tileLayer('https://{s}.basemaps.cartocdn.com/dark_all/{z}/{x}/{y}{r}.png', {
            subdomains: 'abcd', maxZoom: 18
        }).addTo(miniMap);
        L.marker([plant.latitude, plant.longitude]).addTo(miniMap).bindPopup(plant.name).openPopup();
    }, 100);

    // Modbus address boxes
    const plantRegs = modbusInfo.filter(r => r.plant_id === plant.id);
    const varIcons  = {
        'Power':       'fa-bolt text-warning',
        'Voltage':     'fa-plug text-info',
        'Current':     'fa-water text-primary',
        'Frequency':   'fa-wave-square text-success',
        'Temperature': 'fa-thermometer-half text-danger',
        'Status':      'fa-circle text-secondary',
    };
    const mapping = document.getElementById('info-modbus-mapping');
    mapping.innerHTML = '';
    plantRegs.forEach(reg => {
        const varName = detectVar(reg.description);
        const iconCls = varIcons[varName] || varIcons['Status'];
        const col = document.createElement('div');
        col.className = 'col-6 col-md-4 col-xl-2';
        col.innerHTML = `
            <div class="mb-addr-box">
                <i class="fas ${iconCls} fa-fw"></i>
                <div>
                    <div class="mb-ab-label">${varName}</div>
                    <div class="mb-ab-val">${reg.register_address}</div>
                </div>
            </div>`;
        mapping.appendChild(col);
    });
}

// ============================================================
//  HELPERS
// ============================================================
function fmtPower(kw) {
    if (kw >= 1000) return `${(kw / 1000).toFixed(2)} MW`;
    return `${kw.toFixed(2)} kW`;
}

// ============================================================
//  SETTINGS — OFFLINE MODE
// ============================================================
let _settingsModal = null;

function openSettings() {
    if (!_settingsModal) {
        _settingsModal = new bootstrap.Modal(document.getElementById('settingsModal'));
    }
    // Fetch current state from API, then open
    fetch('/api/settings/offline-mode')
        .then(r => r.json())
        .then(data => {
            const toggle = document.getElementById('offlineModeToggle');
            if (toggle) toggle.checked = !!data.offline_mode;
            updateOfflineStatusLine(data.offline_mode);
        })
        .catch(() => {})
        .finally(() => _settingsModal.show());
}

async function toggleOfflineMode(enabled) {
    try {
        const res = await fetch('/api/settings/offline-mode', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ enabled }),
        });
        const data = await res.json();
        updateOfflineBadge(data.offline_mode);
        updateOfflineStatusLine(data.offline_mode);
        updateWeatherSourceBadge(data.offline_mode);
    } catch (e) {
        console.error('toggleOfflineMode:', e);
    }
}

function updateOfflineBadge(isOffline) {
    const badge = document.getElementById('offline-badge');
    if (!badge) return;
    badge.classList.toggle('d-none', !isOffline);
}

function updateOfflineStatusLine(isOffline) {
    const el = document.getElementById('offline-status-line');
    if (!el) return;
    if (isOffline) {
        el.innerHTML = '<span class="text-warning"><i class="fas fa-satellite-dish me-1"></i>'
            + 'Status: <strong>OFFLINE MODE ACTIVE</strong> — solar geometry algorithm running</span>';
    } else {
        el.innerHTML = '<span class="text-success"><i class="fas fa-wifi me-1"></i>'
            + 'Status: <strong>ONLINE MODE ACTIVE</strong> — fetching from Open-Meteo API</span>';
    }
}

function updateWeatherSourceBadge(isOffline) {
    const el = document.getElementById('info-weather-source');
    if (!el) return;
    if (isOffline) {
        el.innerHTML = '<span class="text-warning"><i class="fas fa-satellite-dish me-1"></i> Solar Geometry Algorithm (offline)</span>';
    } else {
        el.innerHTML = '<span class="text-info"><i class="fas fa-cloud me-1"></i> Open-Meteo API (online)</span>';
    }
}

// Poll offline mode every 10 s to keep badge in sync if changed externally
async function syncOfflineModeBadge() {
    try {
        const res = await fetch('/api/settings/offline-mode');
        const data = await res.json();
        updateOfflineBadge(data.offline_mode);
        updateWeatherSourceBadge(data.offline_mode);
    } catch (_) {}
}

// Wire into DOMContentLoaded bootstrap
document.addEventListener('DOMContentLoaded', () => {
    syncOfflineModeBadge();
    setInterval(syncOfflineModeBadge, 10000);
});
