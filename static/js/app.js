// Global State
let plants = [];
let currentPlantId = null;
let currentPlantTimezone = 'UTC';
let map = null;
let markers = {};
let powerChart = null;
let updateInterval = null;

// Initialize
document.addEventListener('DOMContentLoaded', () => {
    initMap();
    initChart();
    fetchPlants();
    startClock();
    
    // Global update loop (every 5 seconds)
    setInterval(updateGlobalData, 5000);
});

function startClock() {
    setInterval(() => {
        const now = new Date();
        document.getElementById('clock').innerText = now.toLocaleTimeString();
    }, 1000);
}

function initMap() {
    // Dark mode map tiles
    map = L.map('map').setView([45.0, 10.0], 5);
    L.tileLayer('https://{s}.basemaps.cartocdn.com/dark_all/{z}/{x}/{y}{r}.png', {
        attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors &copy; <a href="https://carto.com/attributions">CARTO</a>',
        subdomains: 'abcd',
        maxZoom: 19
    }).addTo(map);
}

function initChart() {
    const ctx = document.getElementById('powerChart').getContext('2d');
    powerChart = new Chart(ctx, {
        type: 'line',
        data: {
            labels: [],
            datasets: [{
                label: 'Power Output (kW)',
                data: [],
                borderColor: '#ffc107',
                backgroundColor: 'rgba(255, 193, 7, 0.1)',
                borderWidth: 2,
                fill: true,
                tension: 0.4
            }]
        },
        options: {
            responsive: true,
            maintainAspectRatio: false,
            scales: {
                x: {
                    grid: { color: '#333' },
                    ticks: { color: '#888' }
                },
                y: {
                    grid: { color: '#333' },
                    ticks: { color: '#888' },
                    beginAtZero: true
                }
            },
            plugins: {
                legend: { labels: { color: '#fff' } }
            },
            animation: false
        }
    });
}

async function fetchPlants() {
    try {
        const res = await fetch('/api/plants');
        plants = await res.json();
        renderPlantList();
        renderMapMarkers();
        updateGlobalData();
    } catch (e) {
        console.error("Failed to fetch plants", e);
    }
}

function renderPlantList() {
    const list = document.getElementById('plant-list');
    list.innerHTML = '';
    
    plants.forEach(plant => {
        const div = document.createElement('div');
        div.className = 'plant-item p-3 border-bottom border-secondary';
        div.dataset.id = plant.id;
        div.onclick = () => selectPlant(plant.id);
        div.innerHTML = `
            <div class="d-flex justify-content-between">
                <span class="fw-bold text-white">${plant.name}</span>
                <span class="badge bg-secondary" id="list-power-${plant.id}">-- kW</span>
            </div>
            <small class="text-muted">${plant.id}</small>
        `;
        list.appendChild(div);
    });
    
    document.getElementById('total-plants-count').innerText = plants.length;
}

function renderMapMarkers() {
    plants.forEach(plant => {
        const marker = L.marker([plant.latitude, plant.longitude])
            .addTo(map)
            .bindPopup(`<b>${plant.name}</b><br>Capacity: ${plant.nominal_power_kw} kW<br><button class="btn btn-sm btn-primary mt-2" onclick="selectPlant('${plant.id}')">View Details</button>`);
        markers[plant.id] = marker;
    });
    
    // Fit bounds if plants exist
    if (plants.length > 0) {
        const group = new L.featureGroup(Object.values(markers));
        map.fitBounds(group.getBounds().pad(0.1));
    }
}

async function updateGlobalData() {
    try {
        const res = await fetch('/api/power/global');
        const data = await res.json();
        
        let totalPower = 0;
        let activeCount = 0;
        
        for (const [id, power] of Object.entries(data)) {
            totalPower += power;
            if (power > 0.1) activeCount++;
            
            // Update list item
            const el = document.getElementById(`list-power-${id}`);
            if (el) {
                el.innerText = `${power.toFixed(2)} kW`;
                el.className = power > 0.1 ? 'badge bg-success' : 'badge bg-secondary';
            }
        }
        
        document.getElementById('total-power').innerText = `${totalPower.toFixed(2)} kW`;
        document.getElementById('active-plants-count').innerText = activeCount;
        
    } catch (e) {
        console.error("Error updating global data", e);
    }
}

function selectPlant(id) {
    currentPlantId = id;
    const plant = plants.find(p => p.id === id);
    if (!plant) return;
    
    // UI Switch
    document.getElementById('map-view').classList.add('d-none');
    document.getElementById('detail-view').classList.remove('d-none');
    
    // Highlight list item
    document.querySelectorAll('.plant-item').forEach(el => el.classList.remove('active'));
    const activeItem = document.querySelector(`.plant-item[data-id="${id}"]`);
    if (activeItem) activeItem.classList.add('active');
    
    // Set static details
    document.getElementById('detail-name').innerText = plant.name;
    document.getElementById('detail-id').innerText = `ID: ${plant.id}`;
    document.getElementById('detail-nominal').innerText = `${plant.nominal_power_kw} kW`;
    document.getElementById('detail-location').innerText = `${plant.latitude.toFixed(4)}, ${plant.longitude.toFixed(4)}`;
    
    // Store timezone for clock
    currentPlantTimezone = plant.timezone || 'UTC';
    
    // Clear chart
    powerChart.data.labels = [];
    powerChart.data.datasets[0].data = [];
    powerChart.update();
    
    // Start polling for this plant
    if (updateInterval) clearInterval(updateInterval);
    fetchPlantDetail(); // Immediate
    updateInterval = setInterval(fetchPlantDetail, 2000); // Fast polling for detail view
}

function showMap() {
    currentPlantId = null;
    if (updateInterval) clearInterval(updateInterval);
    
    document.getElementById('map-view').classList.remove('d-none');
    document.getElementById('detail-view').classList.add('d-none');
    document.querySelectorAll('.plant-item').forEach(el => el.classList.remove('active'));
    
    // Refresh map size
    map.invalidateSize();
}

async function fetchPlantDetail() {
    if (!currentPlantId) return;
    
    try {
        const res = await fetch(`/api/plants/${currentPlantId}/power`);
        if (!res.ok) return;
        
        const json = await res.json();
        const data = json.data; // PlantData
        const ts = new Date(json.timestamp);
        
        // Update Cards
        document.getElementById('detail-power').innerText = `${data.power_kw.toFixed(2)} kW`;
        document.getElementById('detail-voltage').innerText = `${data.voltage_v.toFixed(1)} V`;
        document.getElementById('detail-current').innerText = `${data.current_a.toFixed(2)} A`;
        document.getElementById('detail-frequency').innerText = `${data.frequency_hz.toFixed(2)} Hz`;
        document.getElementById('detail-temp').innerText = `${data.temperature_c.toFixed(1)} °C`;
        
        // New fields
        if (document.getElementById('detail-pf')) {
            document.getElementById('detail-pf').innerText = `${data.power_factor.toFixed(2)}`;
        }
        if (document.getElementById('detail-apparent')) {
            document.getElementById('detail-apparent').innerText = `${data.apparent_power_kva.toFixed(2)} kVA`;
        }
        if (document.getElementById('detail-reactive')) {
            document.getElementById('detail-reactive').innerText = `${data.reactive_power_kvar.toFixed(2)} kvar`;
        }

        if (document.getElementById('detail-efficiency')) {
            document.getElementById('detail-efficiency').innerText = `${data.efficiency_percent.toFixed(1)} %`;
        }
        if (document.getElementById('detail-energy')) {
            document.getElementById('detail-energy').innerText = `${data.daily_energy_kwh.toFixed(2)} kWh`;
        }
        
        // Update Local Time
        const now = new Date();
        try {
            const localTime = new Intl.DateTimeFormat('en-US', {
                timeZone: currentPlantTimezone,
                hour: '2-digit',
                minute: '2-digit',
                second: '2-digit',
                hour12: false
            }).format(now);
            document.getElementById('detail-local-time').innerText = `${localTime} (${currentPlantTimezone})`;
        } catch (e) {
            document.getElementById('detail-local-time').innerText = "--:--";
        }

        // Update Weather Icon
        const iconContainer = document.getElementById('weather-icon-container');
        let iconClass = 'fa-sun';
        let iconColor = 'text-warning';
        
        if (!data.is_day) {
            iconClass = 'fa-moon';
            iconColor = 'text-light';
        } else if (data.weather_code > 3) {
            iconClass = 'fa-cloud-sun';
            iconColor = 'text-info';
            if (data.weather_code > 50) {
                iconClass = 'fa-cloud-rain';
                iconColor = 'text-primary';
            }
        }
        
        iconContainer.innerHTML = `<i class="fas ${iconClass} fa-2x ${iconColor}"></i>`;

        const statusEl = document.getElementById('detail-status');
        if (data.status === 1) {
            statusEl.innerText = "RUNNING";
            statusEl.className = "badge bg-success fs-6 pulse";
            document.getElementById('sun-visual').style.opacity = '1';
        } else {
            statusEl.innerText = "STOPPED";
            statusEl.className = "badge bg-secondary fs-6";
            document.getElementById('sun-visual').style.opacity = '0.2';
        }
        
        // Update Chart
        const timeLabel = ts.toLocaleTimeString();
        
        // Keep last 20 points
        if (powerChart.data.labels.length > 20) {
            powerChart.data.labels.shift();
            powerChart.data.datasets[0].data.shift();
        }
        
        powerChart.data.labels.push(timeLabel);
        powerChart.data.datasets[0].data.push(data.power_kw);
        powerChart.update('none'); // 'none' mode for performance
        
    } catch (e) {
        console.error("Error fetching detail", e);
    }
}
