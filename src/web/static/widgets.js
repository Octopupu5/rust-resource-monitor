let data = { xs: [], series: {} };
let followLive = true;
let windowMs = 180000;
let pausedEndTs = null;
let windowStartMs = null;
const tooltip = document.getElementById('tooltip');
let lastView = null;
let hiddenSeries = {};

let widgetOrder = JSON.parse(localStorage.getItem('rm_widgetOrder') || '[]');
let hiddenWidgets = JSON.parse(localStorage.getItem('rm_hiddenWidgets') || '{}');

let selectionStart = null;
let selectionEnd = null;
let isSelecting = false;
let isDraggingWindow = false;
let dragStartX = 0;
let initialWindowStart = 0;
let notificationTimeout = null;

let isResizingLeft = false;
let isResizingRight = false;
let isMoving = false;
let initialWindowMs = 0;

const style = document.createElement('style');
style.textContent = `
    #timeline {
        margin-bottom: 25px !important;
        cursor: crosshair;
    }
    #range-label {
        font-family: ui-monospace, monospace;
        background: #1b2a4a;
        padding: 4px 12px;
        border-radius: 16px;
        border: 1px solid #3b82f6;
        font-size: 12px;
    }
`;
document.head.appendChild(style);

function clamp(x, lo, hi) {
    if (!Number.isFinite(x)) return lo;
    return Math.max(lo, Math.min(hi, x));
}

function fmtTime(ms) {
    const d = new Date(ms);
    const hours = d.getHours().toString().padStart(2, '0');
    const minutes = d.getMinutes().toString().padStart(2, '0');
    const seconds = d.getSeconds().toString().padStart(2, '0');
    return `${hours}:${minutes}:${seconds}`;
}

function fmtDateTime(ms) {
    const d = new Date(ms);
    return d.toLocaleString();
}

function formatValue(value, format) {
    if (!format || value === undefined || value === null) return String(value);

    switch (format.type) {
        case 'Percentage':
            return value.toFixed(format.params?.decimals || 1) + '%';
        case 'Bytes':
            return fmtBytes(value, format.params?.suffix || 'B/s');
        case 'Float':
            return value.toFixed(format.params?.decimals || 2);
        case 'Integer':
            return Math.round(value).toString();
        default:
            return String(value);
    }
}

function fmtBytes(v, suffix) {
    suffix = suffix || 'B/s';
    if (!Number.isFinite(v) || v < 0) return '0 ' + suffix;
    if (v >= 1073741824) return (v / 1073741824).toFixed(2) + ' Gi' + suffix;
    if (v >= 1048576)    return (v / 1048576).toFixed(2)    + ' Mi' + suffix;
    if (v >= 1024)       return (v / 1024).toFixed(1)       + ' Ki' + suffix;
    return v.toFixed(0) + ' ' + suffix;
}

function byteScale(maxY) {
    if (maxY >= 1073741824) return { div: 1073741824, unit: 'GiB/s' };
    if (maxY >= 1048576)    return { div: 1048576,    unit: 'MiB/s' };
    if (maxY >= 1024)       return { div: 1024,       unit: 'KiB/s' };
    return { div: 1, unit: 'B/s' }
}

function getOrderedSeries(snapshotData) {
    const names = snapshotData.map(s => s.name);
    if (widgetOrder.length === 0) {
        widgetOrder = [...names];
        saveWidgetPrefs();
    } else {
        names.forEach(n => { if (!widgetOrder.includes(n)) widgetOrder.push(n); });
        widgetOrder = widgetOrder.filter(n => names.includes(n));
    }
    const byName = {};
    snapshotData.forEach(s => { byName[s.name] = s; });
    return widgetOrder.map(n => byName[n]).filter(Boolean);
}

function saveWidgetPrefs() {
    localStorage.setItem('rm_widgetOrder', JSON.stringify(widgetOrder));
    localStorage.setItem('rm_hiddenWidgets', JSON.stringify(hiddenWidgets));
    updateWidgetMenu();
}

function createChartsFromSnapshot(snapshot) {
    const container = document.getElementById('charts-container');
    if (!container) return;

    container.innerHTML = '';
    const ordered = getOrderedSeries(snapshot.data);

    ordered.forEach(series => {
        if (series.series.length === 0) return;

        const panel = document.createElement('div');
        panel.className = 'panel widget-panel';
        panel.setAttribute('data-series-name', series.name);
        panel.draggable = true;
        if (hiddenWidgets[series.name]) panel.style.display = 'none';

        const header = document.createElement('div');
        header.className = 'widget-header';

        const dragHandle = document.createElement('span');
        dragHandle.className = 'drag-handle';
        dragHandle.textContent = '\u2630';
        dragHandle.title = 'Drag to reorder';

        const title = document.createElement('h3');
        title.style.cssText = 'margin: 0; flex: 1;';
        title.textContent = series.beautiful_name || series.name;

        const fullscreenBtn = document.createElement('button');
        fullscreenBtn.className = 'widget-header-btn';
        fullscreenBtn.innerHTML = '&#x26F6;';
        fullscreenBtn.title = 'Fullscreen';
        fullscreenBtn.addEventListener('click', (e) => {
            e.stopPropagation();
            openFullscreen(series.name);
        });

        const hideBtn = document.createElement('button');
        hideBtn.className = 'widget-hide-btn';
        hideBtn.textContent = '\u00D7';
        hideBtn.title = 'Hide widget';
        hideBtn.addEventListener('click', (e) => {
            e.stopPropagation();
            hiddenWidgets[series.name] = true;
            panel.style.display = 'none';
            saveWidgetPrefs();
        });

        header.appendChild(dragHandle);
        header.appendChild(title);
        header.appendChild(fullscreenBtn);
        header.appendChild(hideBtn);
        panel.appendChild(header);

        const chartDiv = document.createElement('div');
        chartDiv.className = 'chart';

        const canvas = document.createElement('canvas');
        canvas.width = 520;
        canvas.height = 180;
        canvas.id = `chart-${series.name}`;

        const overlay = document.createElement('canvas');
        overlay.width = 520;
        overlay.height = 180;
        overlay.className = 'overlay';
        overlay.id = `chart-${series.name}-ov`;

        chartDiv.appendChild(canvas);
        chartDiv.appendChild(overlay);
        panel.appendChild(chartDiv);

        if (series.legend && series.legend.length > 0) {
            const legend = document.createElement('div');
            legend.style.cssText = 'font-family: ui-monospace, monospace; font-size: 12px; margin-top: 6px; cursor: pointer;';
            legend.innerHTML = series.legend.map((l, idx) => 
                `<span data-series="${series.name}" data-index="${idx}" style="color:${l.color}; opacity: ${hiddenSeries[series.name]?.[idx] ? 0.3 : 1};">${l.name}</span>`
            ).join(' ');
            
            legend.addEventListener('click', (e) => {
                const target = e.target;
                if (target.tagName === 'SPAN') {
                    const seriesName = target.dataset.series;
                    const idx = parseInt(target.dataset.index);
                    
                    if (!hiddenSeries[seriesName]) {
                        hiddenSeries[seriesName] = {};
                    }
                    hiddenSeries[seriesName][idx] = !hiddenSeries[seriesName][idx];
                    
                    target.style.opacity = hiddenSeries[seriesName][idx] ? 0.3 : 1;
                    drawAllCharts();
                }
            });
            
            panel.appendChild(legend);
        }

        panel.addEventListener('dragstart', (e) => {
            e.dataTransfer.effectAllowed = 'move';
            e.dataTransfer.setData('text/plain', series.name);
            panel.classList.add('dragging');
        });
        panel.addEventListener('dragend', () => {
            panel.classList.remove('dragging');
            document.querySelectorAll('.widget-panel.drag-over').forEach(el => el.classList.remove('drag-over'));
        });
        panel.addEventListener('dragover', (e) => {
            e.preventDefault();
            e.dataTransfer.dropEffect = 'move';
            panel.classList.add('drag-over');
        });
        panel.addEventListener('dragleave', () => {
            panel.classList.remove('drag-over');
        });
        panel.addEventListener('drop', (e) => {
            e.preventDefault();
            panel.classList.remove('drag-over');
            const draggedName = e.dataTransfer.getData('text/plain');
            const targetName = series.name;
            if (draggedName === targetName) return;
            const fromIdx = widgetOrder.indexOf(draggedName);
            const toIdx = widgetOrder.indexOf(targetName);
            if (fromIdx < 0 || toIdx < 0) return;
            widgetOrder.splice(fromIdx, 1);
            widgetOrder.splice(toIdx, 0, draggedName);
            saveWidgetPrefs();
            reorderDomWidgets();
        });

        container.appendChild(panel);
    });

    setupChartHandlers();
    updateWidgetMenu();
}

function reorderDomWidgets() {
    const container = document.getElementById('charts-container');
    if (!container) return;
    const panels = Array.from(container.querySelectorAll('.widget-panel'));
    const byName = {};
    panels.forEach(p => { byName[p.getAttribute('data-series-name')] = p; });
    widgetOrder.forEach(name => {
        if (byName[name]) container.appendChild(byName[name]);
    });
}

function updateWidgetMenu() {
    const menu = document.getElementById('widget-menu');
    if (!menu) return;
    menu.innerHTML = '';
    widgetOrder.forEach(name => {
        const seriesData = data.series[name];
        const label = seriesData?.beautiful_name || name;
        const item = document.createElement('label');
        item.className = 'widget-menu-item';
        const cb = document.createElement('input');
        cb.type = 'checkbox';
        cb.checked = !hiddenWidgets[name];
        cb.addEventListener('change', () => {
            if (cb.checked) {
                delete hiddenWidgets[name];
            } else {
                hiddenWidgets[name] = true;
            }
            const panel = document.querySelector(`.widget-panel[data-series-name="${name}"]`);
            if (panel) panel.style.display = hiddenWidgets[name] ? 'none' : '';
            saveWidgetPrefs();
        });
        item.appendChild(cb);
        item.appendChild(document.createTextNode(' ' + label));
        menu.appendChild(item);
    });
}

let fullscreenName = null;

function openFullscreen(seriesName) {
    fullscreenName = seriesName;
    let overlay = document.getElementById('fs-overlay');
    if (!overlay) {
        overlay = document.createElement('div');
        overlay.id = 'fs-overlay';
        overlay.className = 'fs-overlay';
        overlay.innerHTML = `
            <div class="fs-header">
                <h2 id="fs-title"></h2>
                <button id="fs-close" class="widget-header-btn" style="font-size:22px;">&times;</button>
            </div>
            <div class="fs-chart-wrap chart">
                <canvas id="fs-canvas"></canvas>
                <canvas id="fs-canvas-ov" class="overlay"></canvas>
            </div>
            <div id="fs-legend" style="font-family: ui-monospace, monospace; font-size: 13px; margin-top: 8px; cursor: pointer; text-align: center;"></div>
        `;
        document.body.appendChild(overlay);
        document.getElementById('fs-close').addEventListener('click', closeFullscreen);
        overlay.addEventListener('click', (e) => { if (e.target === overlay) closeFullscreen(); });
        document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && fullscreenName) closeFullscreen(); });
        setupFullscreenTooltip();
    }

    const seriesData = data.series[seriesName];
    const title = seriesData?.beautiful_name || seriesName;
    document.getElementById('fs-title').textContent = title;

    overlay.style.display = 'flex';
    document.body.style.overflow = 'hidden';

    drawFullscreenChart();
}

function closeFullscreen() {
    fullscreenName = null;
    const overlay = document.getElementById('fs-overlay');
    if (overlay) overlay.style.display = 'none';
    document.body.style.overflow = '';
    tooltip.style.display = 'none';
}

function drawFullscreenChart() {
    if (!fullscreenName) return;
    const view = getCurrentView();
    if (!view || view.xs.length === 0) return;

    const name = fullscreenName;
    const seriesData = view.series[name];
    if (!seriesData) return;

    const canvas = document.getElementById('fs-canvas');
    const wrap = canvas.parentElement;
    const newW = wrap.clientWidth;
    const newH = wrap.clientHeight;
    if (canvas.width !== newW || canvas.height !== newH) {
        canvas.width = newW;
        canvas.height = newH;
        const ovCanvas = document.getElementById('fs-canvas-ov');
        ovCanvas.width = newW;
        ovCanvas.height = newH;
    }

    const seriesList = [];
    let maxLines = 0;
    for (let i = view.startIdx; i < view.endIdx; i++) {
        maxLines = Math.max(maxLines, (seriesData.values[i] || []).length);
    }
    for (let lineIdx = 0; lineIdx < maxLines; lineIdx++) {
        if (hiddenSeries[name]?.[lineIdx]) continue;
        const ys = [];
        let color = '#888';
        for (let i = view.endIdx - 1; i >= view.startIdx; i--) {
            const legends = seriesData.legends[i] || [];
            if (legends[lineIdx]) { color = legends[lineIdx].color; break; }
        }
        for (let j = view.startIdx; j < view.endIdx; j++) {
            ys.push(seriesData.values[j]?.[lineIdx] || 0);
        }
        seriesList.push({ ys, color, lineWidth: maxLines > 8 ? 1.5 : 2.5, lineIdx });
    }

    let minY = 0, maxY = 100;
    if (seriesData.format) {
        if (seriesData.format.type === 'Bytes' || seriesData.format.type !== 'Percentage') {
            maxY = Math.max(1, ...seriesList.flatMap(s => s.ys)) * 1.1;
        }
    }

    drawLineChart(canvas, seriesList, {
        xs: view.xs, minY, maxY,
        byteY: seriesData.format?.type === 'Bytes',
        seriesName: name, seriesData, startIdx: view.startIdx,
        warn: seriesData.warn, crit: seriesData.crit
    });

    const legendDiv = document.getElementById('fs-legend');
    if (legendDiv && seriesData.legends.length > 0) {
        const latestLegends = seriesData.legends[seriesData.legends.length - 1] || [];
        legendDiv.innerHTML = latestLegends.map((l, idx) =>
            `<span data-series="${name}" data-index="${idx}" style="color:${l.color}; opacity: ${hiddenSeries[name]?.[idx] ? 0.3 : 1}; margin: 0 6px;">${l.name}${l.comment ? ' (' + l.comment + ')' : ''}</span>`
        ).join('');
    }
}

function setupFullscreenTooltip() {
    const ovCanvas = document.getElementById('fs-canvas-ov');
    if (!ovCanvas) return;
    ovCanvas.style.pointerEvents = 'auto';

    ovCanvas.addEventListener('mousemove', (e) => {
        if (!fullscreenName) return;
        const canvas = document.getElementById('fs-canvas');
        const meta = canvas?.__meta;
        if (!meta) return;

        const rect = ovCanvas.getBoundingClientRect();
        const mx = e.clientX - rect.left;
        const my = e.clientY - rect.top;

        const ctx = ovCanvas.getContext('2d');
        ctx.clearRect(0, 0, ovCanvas.width, ovCanvas.height);

        const name = fullscreenName;
        const seriesData = data.series[name];
        if (!seriesData) return;

        const xScale = (meta.maxX === meta.minX) ? 0 : (meta.w - meta.leftPad - meta.rightPad) / (meta.maxX - meta.minX);
        const yScale = (meta.maxY === meta.minY) ? 0 : (meta.h - meta.topPad - meta.bottomPad) / (meta.maxY - meta.minY);

        let nearestIdx = 0, nearestDist = Infinity;
        const view = getCurrentView();
        if (!view) return;
        for (let i = 0; i < view.xs.length; i++) {
            const px = meta.leftPad + (view.xs[i] - meta.minX) * xScale;
            const dist = Math.abs(px - mx);
            if (dist < nearestDist) { nearestDist = dist; nearestIdx = i; }
        }
        if (nearestDist > 40) { tooltip.style.display = 'none'; return; }

        const nearestXPos = meta.leftPad + (view.xs[nearestIdx] - meta.minX) * xScale;
        ctx.strokeStyle = 'rgba(255,255,255,0.3)';
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(nearestXPos, 0);
        ctx.lineTo(nearestXPos, ovCanvas.height);
        ctx.stroke();

        const dataIdx = view.startIdx + nearestIdx;
        const rows = [`<div style="color:#9ca3af; margin-bottom:4px;">${fmtTime(view.xs[nearestIdx])}</div>`];
        const pointLegends = seriesData.legends[dataIdx] || [];
        for (let j = 0; j < pointLegends.length; j++) {
            if (hiddenSeries[name]?.[j]) continue;
            const legend = pointLegends[j];
            const v = seriesData.values[dataIdx]?.[j] || 0;
            const yPos = meta.topPad + (meta.h - meta.topPad - meta.bottomPad) - (v - meta.minY) * yScale;
            ctx.fillStyle = legend.color;
            ctx.beginPath();
            ctx.arc(nearestXPos, yPos, 5, 0, Math.PI * 2);
            ctx.fill();
            let formattedValue = formatValue(v, seriesData.format);
            let displayText = `<span style="color:${legend.color};">${legend.name}</span>: ${formattedValue}`;
            if (legend.comment) displayText += ` <span style="color:#9ca3af; font-size:11px;">(${legend.comment})</span>`;
            rows.push(`<div style="margin:2px 0;">${displayText}</div>`);
        }
        tooltip.innerHTML = rows.join('');
        tooltip.style.display = 'block';
        const tx = e.clientX + 14;
        const ty = e.clientY + 14;
        tooltip.style.left = Math.min(tx, window.innerWidth - 300) + 'px';
        tooltip.style.top = Math.min(ty, window.innerHeight - 200) + 'px';
    });

    ovCanvas.addEventListener('mouseleave', () => {
        const ctx = ovCanvas.getContext('2d');
        ctx.clearRect(0, 0, ovCanvas.width, ovCanvas.height);
        tooltip.style.display = 'none';
    });
}

function pushDataPoint(rpcSnapshot) {
    const ts = rpcSnapshot.timestamp_ms;
    if (typeof ts !== 'number') return;

    const last = data.xs.length ? data.xs[data.xs.length - 1] : 0;
    if (ts <= last) return;

    data.xs.push(ts);

    rpcSnapshot.data.forEach(series => {
        if (!data.series[series.name]) {
            data.series[series.name] = {
                values: [],
                legends: [],
                format: series.format,
                beautiful_name: series.beautiful_name,
                warn: series.warn ?? null,
                crit: series.crit ?? null
            };
        }
        data.series[series.name].values.push(series.series);
        data.series[series.name].legends.push(series.legend);
    });

    const maxLen = 20000;
    if (data.xs.length > maxLen) {
        const drop = data.xs.length - maxLen;
        data.xs.splice(0, drop);
        Object.keys(data.series).forEach(name => {
            data.series[name].values.splice(0, drop);
            data.series[name].legends.splice(0, drop);
        });
    }

    updateEndSlider();
}

function resetData() {
    data = { xs: [], series: {} };
}

function getCurrentView() {
    if (data.xs.length === 0) return null;

    const endTs = followLive ? data.xs[data.xs.length - 1] : (pausedEndTs ?? data.xs[data.xs.length - 1]);
    const startTs = windowMs === 0 ? data.xs[0] : Math.max(data.xs[0], endTs - windowMs);

    windowStartMs = startTs;
    let startIdx = 0;
    let endIdx = data.xs.length - 1;

    for (let i = 0; i < data.xs.length; i++) {
        if (data.xs[i] >= startTs) {
            startIdx = i;
            break;
        }
    }

    for (let i = data.xs.length - 1; i >= 0; i--) {
        if (data.xs[i] <= endTs) {
            endIdx = i + 1;
            break;
        }
    }

    return {
        xs: data.xs.slice(startIdx, endIdx),
        series: data.series,
        startIdx,
        endIdx,
        startTs,
        endTs
    };
}

function drawAllCharts() {
    const view = getCurrentView();
    if (!view || view.xs.length === 0) return;

    lastView = view;

    document.querySelectorAll('canvas.overlay').forEach(canvas => {
        const ctx = canvas.getContext('2d');
        ctx.clearRect(0, 0, canvas.width, canvas.height);
    });

    tooltip.style.display = 'none';

    Object.keys(view.series).forEach(name => {
        const canvas = document.getElementById(`chart-${name}`);
        if (!canvas) return;

        const seriesData = view.series[name];
        const seriesList = [];

        let maxLines = 0;
        for (let i = view.startIdx; i < view.endIdx; i++) {
            const values = seriesData.values[i] || [];
            maxLines = Math.max(maxLines, values.length);
        }

        for (let lineIdx = 0; lineIdx < maxLines; lineIdx++) {
            if (hiddenSeries[name]?.[lineIdx]) continue;

            const ys = [];
            let color = '#888';
            for (let i = view.endIdx - 1; i >= view.startIdx; i--) {
                const legends = seriesData.legends[i] || [];
                if (legends[lineIdx]) {
                    color = legends[lineIdx].color;
                    break;
                }
            }
            
            for (let j = view.startIdx; j < view.endIdx; j++) {
                ys.push(seriesData.values[j]?.[lineIdx] || 0);
            }
            
            seriesList.push({
                ys: ys,
                color: color,
                lineWidth: maxLines > 8 ? 1 : 2,
                lineIdx: lineIdx
            });
        }

        let minY = 0;
        let maxY = 100;

        if (seriesData.format) {
            if (seriesData.format.type === 'Bytes') {
                maxY = Math.max(1, ...seriesList.flatMap(s => s.ys)) * 1.1;
            } else if (seriesData.format.type !== 'Percentage') {
                maxY = Math.max(1, ...seriesList.flatMap(s => s.ys)) * 1.1;
            }
        }

        drawLineChart(canvas, seriesList, {
            xs: view.xs,
            minY: minY,
            maxY: maxY,
            byteY: seriesData.format?.type === 'Bytes',
            seriesName: name,
            seriesData: seriesData,
            startIdx: view.startIdx,
            warn: seriesData.warn,
            crit: seriesData.crit
        });
    });

    updateRangeLabel(view);
    drawTimeline();
}

function drawLineChart(canvas, series, options) {
    const ctx = canvas.getContext('2d');
    const w = canvas.width, h = canvas.height;
    ctx.clearRect(0, 0, w, h);

    ctx.fillStyle = '#0f1626';
    ctx.fillRect(0, 0, w, h);

    ctx.strokeStyle = '#1c2740';
    ctx.lineWidth = 1;
    for (let i = 0; i <= 4; i++) {
        const y = (h * i) / 4;
        ctx.beginPath();
        ctx.moveTo(0, y);
        ctx.lineTo(w, y);
        ctx.stroke();
    }

    const xs = options.xs;
    const minX = xs[0];
    const maxX = xs[xs.length - 1];
    const minY = options.minY;
    const maxY = options.maxY;
    const leftPad = 50;
    const rightPad = 40;
    const topPad = 10;
    const bottomPad = 20;

    const warnVal = options.warn;
    const critVal = options.crit;
    if (warnVal != null || critVal != null) {
        const chartH = h - topPad - bottomPad;
        const chartW = w - leftPad - rightPad;
        const valToY = (v) => topPad + chartH * (1 - (v - minY) / (maxY - minY));
        const bottomY = topPad + chartH;

        ctx.setLineDash([6, 4]);
        ctx.lineWidth = 1;

        if (warnVal != null && critVal != null && critVal > warnVal) {
            const warnY = valToY(warnVal);
            const critY = valToY(critVal);
            ctx.fillStyle = 'rgba(250, 204, 21, 0.06)';
            ctx.fillRect(leftPad, warnY, chartW, bottomY - warnY);
            ctx.fillStyle = 'rgba(239, 68, 68, 0.08)';
            ctx.fillRect(leftPad, critY, chartW, warnY - critY);

            ctx.strokeStyle = 'rgba(250, 204, 21, 0.35)';
            ctx.beginPath(); ctx.moveTo(leftPad, warnY); ctx.lineTo(w - rightPad, warnY); ctx.stroke();
            ctx.strokeStyle = 'rgba(239, 68, 68, 0.45)';
            ctx.beginPath(); ctx.moveTo(leftPad, critY); ctx.lineTo(w - rightPad, critY); ctx.stroke();

            ctx.setLineDash([]);
            ctx.font = '9px ui-monospace, monospace';
            ctx.textBaseline = 'bottom';
            ctx.textAlign = 'right';
            ctx.fillStyle = 'rgba(250, 204, 21, 0.6)';
            ctx.fillText(`warn ${warnVal}%`, w - rightPad - 2, warnY - 2);
            ctx.fillStyle = 'rgba(239, 68, 68, 0.7)';
            ctx.fillText(`crit ${critVal}%`, w - rightPad - 2, critY - 2);
        } else if (warnVal != null && critVal != null && critVal < warnVal) {
            const warnY = valToY(warnVal);
            const critY = valToY(critVal);
            ctx.fillStyle = 'rgba(250, 204, 21, 0.06)';
            ctx.fillRect(leftPad, topPad, chartW, warnY - topPad);
            ctx.fillStyle = 'rgba(239, 68, 68, 0.08)';
            ctx.fillRect(leftPad, warnY, chartW, critY - warnY);

            ctx.strokeStyle = 'rgba(250, 204, 21, 0.35)';
            ctx.beginPath(); ctx.moveTo(leftPad, warnY); ctx.lineTo(w - rightPad, warnY); ctx.stroke();
            ctx.strokeStyle = 'rgba(239, 68, 68, 0.45)';
            ctx.beginPath(); ctx.moveTo(leftPad, critY); ctx.lineTo(w - rightPad, critY); ctx.stroke();

            ctx.setLineDash([]);
            ctx.font = '9px ui-monospace, monospace';
            ctx.textBaseline = 'top';
            ctx.textAlign = 'right';
            ctx.fillStyle = 'rgba(250, 204, 21, 0.6)';
            ctx.fillText(`warn ${warnVal}%`, w - rightPad - 2, warnY + 2);
            ctx.fillStyle = 'rgba(239, 68, 68, 0.7)';
            ctx.fillText(`crit ${critVal}%`, w - rightPad - 2, critY + 2);
        } else {
            if (warnVal != null) {
                const wy = valToY(warnVal);
                ctx.strokeStyle = 'rgba(250, 204, 21, 0.35)';
                ctx.beginPath(); ctx.moveTo(leftPad, wy); ctx.lineTo(w - rightPad, wy); ctx.stroke();
                ctx.setLineDash([]);
                ctx.font = '9px ui-monospace, monospace'; ctx.textBaseline = 'bottom'; ctx.textAlign = 'right';
                ctx.fillStyle = 'rgba(250, 204, 21, 0.6)';
                ctx.fillText(`warn ${warnVal}%`, w - rightPad - 2, wy - 2);
            }
            if (critVal != null) {
                const cy = valToY(critVal);
                ctx.strokeStyle = 'rgba(239, 68, 68, 0.45)';
                ctx.beginPath(); ctx.moveTo(leftPad, cy); ctx.lineTo(w - rightPad, cy); ctx.stroke();
                ctx.setLineDash([]);
                ctx.font = '9px ui-monospace, monospace'; ctx.textBaseline = 'bottom'; ctx.textAlign = 'right';
                ctx.fillStyle = 'rgba(239, 68, 68, 0.7)';
                ctx.fillText(`crit ${critVal}%`, w - rightPad - 2, cy - 2);
            }
        }
        ctx.setLineDash([]);
    }

    canvas.__meta = { 
        minX, 
        maxX, 
        minY, 
        maxY, 
        w, 
        h, 
        leftPad, 
        rightPad, 
        topPad, 
        bottomPad,
        seriesName: options.seriesName,
        seriesData: options.seriesData,
        startIdx: options.startIdx
    };

    const xScale = (maxX === minX) ? 0 : (w - leftPad - rightPad) / (maxX - minX);
    const yScale = (maxY === minY) ? 0 : (h - topPad - bottomPad) / (maxY - minY);

    function xToPx(x) {
        return leftPad + (x - minX) * xScale;
    }

    function yToPx(y) {
        return topPad + (h - topPad - bottomPad) - (y - minY) * yScale;
    }

    ctx.fillStyle = '#9ca3af';
    ctx.font = '10px monospace';
    ctx.textAlign = 'left';
    ctx.textBaseline = 'middle';

    const yTicks = 4;
    const scale = options.byteY ? byteScale(maxY) : null;
    for (let i = 0; i < yTicks; i++) {
        const v = minY + (maxY - minY) * (i / (yTicks - 1));
        const py = yToPx(v);
        let label;
        if (scale) {
            label = (v / scale.div).toFixed(1) + ' ' + scale.unit;
        } else {
            label = Math.round(v) + (options.byteY ? '' : '%');
        }
        ctx.fillText(label, 4, py);
    }

    ctx.textBaseline = 'alphabetic';
    const xTicks = 5;
    for (let i = 0; i < xTicks; i++) {
        const ts = minX + (maxX - minX) * (i / (xTicks - 1));
        const px = xToPx(ts);
        const txt = fmtTime(ts);
        ctx.fillText(txt, px - 25, h - 6);
    }

    for (const s of series) {
        ctx.strokeStyle = s.color;
        ctx.lineWidth = s.lineWidth || 2;
        ctx.beginPath();
        
        if (s.ys.length > 0) {
            ctx.moveTo(xToPx(xs[0]), yToPx(s.ys[0]));
            for (let i = 1; i < xs.length; i++) {
                ctx.lineTo(xToPx(xs[i]), yToPx(s.ys[i]));
            }
        }
        ctx.stroke();

        if (xs.length < 200) {
            ctx.fillStyle = s.color;
            for (let i = 0; i < xs.length; i++) {
                ctx.beginPath();
                ctx.arc(xToPx(xs[i]), yToPx(s.ys[i]), 2, 0, Math.PI * 2);
                ctx.fill();
            }
        }
    }
}

function drawTimeline() {
    const tl = document.getElementById('timeline');
    if (!tl || data.xs.length < 2) return;

    const rect = tl.getBoundingClientRect();
    tl.width = rect.width;
    tl.height = rect.height;

    const ctx = tl.getContext('2d');
    const w = tl.width, h = tl.height;
    ctx.clearRect(0, 0, w, h);

    ctx.fillStyle = '#0f1626';
    ctx.fillRect(0, 0, w, h);

    const minX = data.xs[0];
    const maxX = data.xs[data.xs.length - 1];
    const pad = 0;
    const timeLabelArea = 35;

    if (data.series['cpu_total'] && data.series['cpu_total'].values.length > 0) {
        ctx.strokeStyle = 'rgba(196, 68, 68, 0.5)';
        ctx.lineWidth = 1;
        ctx.beginPath();

        const step = Math.max(1, Math.floor(data.xs.length / 200));
        let first = true;

        for (let i = 0; i < data.xs.length; i += step) {
            const x = pad + ((data.xs[i] - minX) / (maxX - minX)) * (w - 2 * pad);
            const chartTop = 14;
            const chartBot = h - timeLabelArea - 4;
            const y = chartTop + (1 - (data.series['cpu_total'].values[i][0] || 0) / 100) * (chartBot - chartTop);

            if (first) {
                ctx.moveTo(x, y);
                first = false;
            } else {
                ctx.lineTo(x, y);
            }
        }
        ctx.stroke();
    }

    ctx.strokeStyle = '#2a3550';
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(pad, h - timeLabelArea);
    ctx.lineTo(w - pad, h - timeLabelArea);
    ctx.stroke();
    if (windowMs > 0 && windowStartMs) {
        const x0 = pad + ((windowStartMs - minX) / (maxX - minX)) * (w - 2 * pad);
        const x1 = pad + ((windowStartMs + windowMs - minX) / (maxX - minX)) * (w - 2 * pad);

        const clampedX0 = Math.max(pad, Math.min(w - pad, x0));
        const clampedX1 = Math.max(pad, Math.min(w - pad, x1));

        ctx.fillStyle = 'rgba(59, 130, 246, 0.2)';
        ctx.fillRect(clampedX0, 0, clampedX1 - clampedX0, h - timeLabelArea);

        ctx.strokeStyle = '#3b82f6';
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(clampedX0, 0); ctx.lineTo(clampedX0, h - timeLabelArea);
        ctx.moveTo(clampedX1, 0); ctx.lineTo(clampedX1, h - timeLabelArea);
        ctx.stroke();
    }

    if (selectionStart !== null && selectionEnd !== null) {
        const xStart = Math.max(pad, Math.min(w - pad, Math.min(selectionStart, selectionEnd)));
        const xEnd = Math.max(pad, Math.min(w - pad, Math.max(selectionStart, selectionEnd)));

        if (xEnd > xStart) {
            ctx.fillStyle = 'rgba(236, 72, 153, 0.15)';
            ctx.fillRect(xStart, 0, xEnd - xStart, h - timeLabelArea);
            ctx.strokeStyle = '#ec4899';
            ctx.lineWidth = 2;
            ctx.setLineDash([5, 5]);
            ctx.beginPath();
            ctx.moveTo(xStart, 0); ctx.lineTo(xStart, h - timeLabelArea);
            ctx.moveTo(xEnd, 0); ctx.lineTo(xEnd, h - timeLabelArea);
            ctx.stroke();
            ctx.setLineDash([]);
        }
    }

    ctx.font = '10px ui-monospace, monospace';
    ctx.fillStyle = '#9ca3af';
    ctx.textBaseline = 'top';
    
    const timeLabels = 8;
    for (let i = 0; i <= timeLabels; i++) {
        const x = pad + (i / timeLabels) * (w - 2 * pad);
        const time = minX + (i / timeLabels) * (maxX - minX);
        const timeStr = fmtTime(time);

        ctx.strokeStyle = '#2a3550';
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(x, h - timeLabelArea);
        ctx.lineTo(x, h - timeLabelArea + 5);
        ctx.stroke();
        ctx.fillStyle = '#9ca3af';
        if (i === 0) {
            ctx.textAlign = 'left';
        } else if (i === timeLabels) {
            ctx.textAlign = 'right';
        } else {
            ctx.textAlign = 'center';
        }
        ctx.fillText(timeStr, x, h - timeLabelArea + 8);
    }

    const chartMidY = Math.round((h - timeLabelArea) / 2);

    if (windowMs > 0 && windowStartMs) {
        const x0 = pad + ((windowStartMs - minX) / (maxX - minX)) * (w - 2 * pad);
        const x1 = pad + ((windowStartMs + windowMs - minX) / (maxX - minX)) * (w - 2 * pad);

        const clampedX0 = Math.max(pad, Math.min(w - pad, x0));
        const clampedX1 = Math.max(pad, Math.min(w - pad, x1));
        ctx.font = 'bold 12px ui-monospace, monospace';
        ctx.fillStyle = '#e0f2fe';
        ctx.textBaseline = 'middle';

        ctx.textAlign = 'left';
        ctx.fillText(fmtTime(windowStartMs), clampedX0 + 4, chartMidY);
        ctx.textAlign = 'right';
        ctx.fillText(fmtTime(windowStartMs + windowMs), clampedX1 - 4, chartMidY);
    }

    if (selectionStart !== null && selectionEnd !== null && selectionEnd - selectionStart > 20) {
        const xStart = Math.max(pad, Math.min(w - pad, Math.min(selectionStart, selectionEnd)));
        const xEnd = Math.max(pad, Math.min(w - pad, Math.max(selectionStart, selectionEnd)));
        const minX = data.xs[0];
        const maxX = data.xs[data.xs.length - 1];

        const timeStart = minX + ((xStart - pad) / (w - 2 * pad)) * (maxX - minX);
        const timeEnd = minX + ((xEnd - pad) / (w - 2 * pad)) * (maxX - minX);

        ctx.font = 'bold 12px ui-monospace, monospace';
        ctx.fillStyle = '#fce7f3';
        ctx.textBaseline = 'middle';

        ctx.textAlign = 'left';
        ctx.fillText(fmtTime(timeStart), xStart + 4, chartMidY);
        ctx.textAlign = 'right';
        ctx.fillText(fmtTime(timeEnd), xEnd - 4, chartMidY);
    }

}

function updateStatCards(snapshot) {
    const container = document.getElementById('stat-cards');
    if (!container) return;

    container.innerHTML = '';

    snapshot.data.forEach(series => {
        if (series.series.length === 0) return;

        const card = document.createElement('div');
        card.className = 'stat-card panel';

        const label = document.createElement('div');
        label.className = 'stat-label';
        label.textContent = series.beautiful_name || series.name;
        card.appendChild(label);

        const value = document.createElement('div');
        value.className = 'stat-val';
        value.textContent = formatValue(series.series[0], series.format);
        card.appendChild(value);

        container.appendChild(card);
    });
}

function setupChartHandlers() {
    document.querySelectorAll('.chart').forEach(chartDiv => {
        const baseCanvas = chartDiv.querySelector('canvas:not(.overlay)');
        const overlayCanvas = chartDiv.querySelector('canvas.overlay');
        if (!baseCanvas || !overlayCanvas) return;
        
        const seriesName = baseCanvas.id.replace('chart-', '');
        
        function clearTooltip() {
            const ctx = overlayCanvas.getContext('2d');
            ctx.clearRect(0, 0, overlayCanvas.width, overlayCanvas.height);
            tooltip.style.display = 'none';
        }
        
        baseCanvas.addEventListener('mouseleave', clearTooltip);
        
        baseCanvas.addEventListener('mousemove', (e) => {
            if (!lastView || !baseCanvas.__meta) return;

            const rect = baseCanvas.getBoundingClientRect();
            const meta = baseCanvas.__meta;

            const scaleX = meta.w / rect.width;
            const mouseCanvasX = (e.clientX - rect.left) * scaleX;

            if (mouseCanvasX < meta.leftPad || mouseCanvasX > meta.w - meta.rightPad) {
                clearTooltip();
                return;
            }

            const xs = lastView.xs;
            const xPositions = [];

            for (let i = 0; i < xs.length; i++) {
                const timeRatio = (xs[i] - meta.minX) / (meta.maxX - meta.minX);
                const xPos = meta.leftPad + timeRatio * (meta.w - meta.leftPad - meta.rightPad);
                xPositions.push(xPos);
            }

            let nearestIdx = 0;
            let minDist = Math.abs(xPositions[0] - mouseCanvasX);

            for (let i = 1; i < xPositions.length; i++) {
                const dist = Math.abs(xPositions[i] - mouseCanvasX);
                if (dist < minDist) {
                    minDist = dist;
                    nearestIdx = i;
                }
            }

            const nearestXPos = xPositions[nearestIdx];
            const dataIdx = nearestIdx + meta.startIdx;

            const ctx = overlayCanvas.getContext('2d');
            ctx.clearRect(0, 0, overlayCanvas.width, overlayCanvas.height);

            ctx.strokeStyle = 'rgba(156, 163, 175, 0.5)';
            ctx.lineWidth = 1;
            ctx.beginPath();
            ctx.moveTo(nearestXPos, 0);
            ctx.lineTo(nearestXPos, overlayCanvas.height);
            ctx.stroke();

            if (selectionStart !== null && selectionEnd !== null) {
                const rect = document.getElementById('timeline').getBoundingClientRect();
                const pad = 0;

                ctx.strokeStyle = '#ec4899';
                ctx.lineWidth = 1;
                ctx.setLineDash([5, 5]);

                const minX = meta.minX;
                const maxX = meta.maxX;

                const xStart = Math.min(selectionStart, selectionEnd);
                const xEnd = Math.max(selectionStart, selectionEnd);

                const timeStart = minX + ((xStart - pad) / (rect.width - 2 * pad)) * (maxX - minX);
                const timeEnd = minX + ((xEnd - pad) / (rect.width - 2 * pad)) * (maxX - minX);

                if (timeStart >= meta.minX && timeStart <= meta.maxX) {
                    const xPosStart = meta.leftPad + ((timeStart - meta.minX) / (meta.maxX - meta.minX)) * (meta.w - meta.leftPad - meta.rightPad);
                    ctx.beginPath();
                    ctx.moveTo(xPosStart, 0);
                    ctx.lineTo(xPosStart, overlayCanvas.height);
                    ctx.stroke();
                }

                if (timeEnd >= meta.minX && timeEnd <= meta.maxX) {
                    const xPosEnd = meta.leftPad + ((timeEnd - meta.minX) / (meta.maxX - meta.minX)) * (meta.w - meta.leftPad - meta.rightPad);
                    ctx.beginPath();
                    ctx.moveTo(xPosEnd, 0);
                    ctx.lineTo(xPosEnd, overlayCanvas.height);
                    ctx.stroke();
                }

                ctx.setLineDash([]);
            }

            const seriesData = lastView.series[seriesName];
            if (!seriesData) return;

            const rows = [`<div style="color:#9ca3af; margin-bottom: 4px;">${fmtTime(xs[nearestIdx])}</div>`];

            const pointLegends = seriesData.legends[dataIdx] || [];
            
            for (let j = 0; j < pointLegends.length; j++) {
                if (hiddenSeries[seriesName]?.[j]) continue;
                
                const legend = pointLegends[j];
                const v = seriesData.values[dataIdx]?.[j] || 0;

                const valueRatio = (v - meta.minY) / (meta.maxY - meta.minY);
                const yPos = meta.topPad + (meta.h - meta.topPad - meta.bottomPad) - 
                            valueRatio * (meta.h - meta.topPad - meta.bottomPad);

                ctx.fillStyle = legend.color;
                ctx.beginPath();
                ctx.arc(nearestXPos, yPos, 5, 0, Math.PI * 2);
                ctx.fill();
                
                ctx.strokeStyle = '#ffffff';
                ctx.lineWidth = 2;
                ctx.beginPath();
                ctx.arc(nearestXPos, yPos, 5, 0, Math.PI * 2);
                ctx.stroke();

                let formattedValue = formatValue(v, seriesData.format);

                let displayText = `<span style="color:${legend.color};">${legend.name}</span>: ${formattedValue}`;
                if (legend.comment) {
                    displayText += ` <span style="color:#9ca3af; font-size: 11px;">(${legend.comment})</span>`;
                }
                
                rows.push(`<div style="margin: 2px 0;">${displayText}</div>`);
            }
            
            tooltip.innerHTML = rows.join('');
            tooltip.style.display = 'block';

            const tw = tooltip.offsetWidth;
            const th = tooltip.offsetHeight;
            let left = e.clientX + 12;
            let top = e.clientY + 12;

            if (left + tw > window.innerWidth - 8) left = e.clientX - tw - 12;
            if (top + th > window.innerHeight - 8) top = e.clientY - th - 12;

            tooltip.style.left = left + 'px';
            tooltip.style.top = top + 'px';
        });
    });
}

function setupTimelineDrag() {
    const tl = document.getElementById('timeline');
    if (!tl) return;

    tl.addEventListener('mousedown', (e) => {
        if (data.xs.length < 2) return;
        const rect = tl.getBoundingClientRect();
        const x = e.clientX - rect.left;
        const pad = 0;
        const timeLabelArea = 35;

        if (e.clientY > rect.bottom - timeLabelArea) {
            return;
        }

        const clampedX = Math.max(pad, Math.min(rect.width - pad, x));

        if (windowMs > 0 && windowStartMs) {
            const minX = data.xs[0];
            const maxX = data.xs[data.xs.length - 1];
            const x0 = pad + ((windowStartMs - minX) / (maxX - minX)) * (rect.width - 2 * pad);
            const x1 = pad + ((windowStartMs + windowMs - minX) / (maxX - minX)) * (rect.width - 2 * pad);

            const edgeSize = 8;
            if (Math.abs(clampedX - x0) < edgeSize) {
                isDraggingWindow = true;
                isResizingLeft = true;
                dragStartX = clampedX;
                initialWindowStart = windowStartMs;
                initialWindowMs = windowMs;
                tl.style.cursor = 'ew-resize';
                return;
            } else if (Math.abs(clampedX - x1) < edgeSize) {
                isDraggingWindow = true;
                isResizingRight = true;
                dragStartX = clampedX;
                initialWindowStart = windowStartMs;
                initialWindowMs = windowMs;
                tl.style.cursor = 'ew-resize';
                return;
            } else if (clampedX >= x0 && clampedX <= x1) {
                isDraggingWindow = true;
                isMoving = true;
                dragStartX = clampedX;
                initialWindowStart = windowStartMs;
                tl.style.cursor = 'grabbing';
                return;
            }
        }

        isSelecting = true;
        selectionStart = clampedX;
        selectionEnd = clampedX;
        tl.style.cursor = 'crosshair';
    });

    window.addEventListener('mousemove', (e) => {
        if (!isDraggingWindow && !isSelecting) {
            if (data.xs.length >= 2 && windowMs > 0 && windowStartMs) {
                const rect = tl.getBoundingClientRect();
                const x = e.clientX - rect.left;
                const pad = 0;
                
                const minX = data.xs[0];
                const maxX = data.xs[data.xs.length - 1];
                const x0 = pad + ((windowStartMs - minX) / (maxX - minX)) * (rect.width - 2 * pad);
                const x1 = pad + ((windowStartMs + windowMs - minX) / (maxX - minX)) * (rect.width - 2 * pad);
                
                const edgeSize = 8;
                if (Math.abs(x - x0) < edgeSize || Math.abs(x - x1) < edgeSize) {
                    tl.style.cursor = 'ew-resize';
                } else if (x >= x0 && x <= x1) {
                    tl.style.cursor = 'grab';
                } else {
                    tl.style.cursor = 'crosshair';
                }
            }
            return;
        }
        
        if (data.xs.length < 2) return;

        const rect = tl.getBoundingClientRect();
        const currentX = e.clientX - rect.left;
        const pad = 0;
        const clampedX = Math.max(pad, Math.min(rect.width - pad, currentX));

        if (isDraggingWindow) {
            const minX = data.xs[0];
            const maxX = data.xs[data.xs.length - 1];
            const timeRange = maxX - minX;
            
            if (isResizingLeft) {
                const deltaX = dragStartX - clampedX;
                const deltaRatio = deltaX / (rect.width - 2 * pad);
                const deltaTime = deltaRatio * timeRange;
                
                let newStart = initialWindowStart - deltaTime;
                let newEnd = initialWindowStart + initialWindowMs;
                
                newStart = clamp(newStart, minX, newEnd - 60000);
                windowMs = newEnd - newStart;
                windowStartMs = newStart;
            } else if (isResizingRight) {
                const deltaX = clampedX - dragStartX;
                const deltaRatio = deltaX / (rect.width - 2 * pad);
                const deltaTime = deltaRatio * timeRange;
                
                let newEnd = initialWindowStart + initialWindowMs + deltaTime;
                newEnd = clamp(newEnd, initialWindowStart + 60000, maxX);
                windowMs = newEnd - initialWindowStart;
                windowStartMs = initialWindowStart;
            } else if (isMoving) {
                const deltaX = clampedX - dragStartX;
                const deltaRatio = deltaX / (rect.width - 2 * pad);
                
                let newStart = initialWindowStart + deltaRatio * timeRange;
                newStart = clamp(newStart, minX, maxX - windowMs);
                windowStartMs = newStart;
            }
            
            pausedEndTs = windowStartMs + windowMs;
            followLive = (pausedEndTs >= maxX - 1000);
            
            updateEndSlider();
            drawAllCharts();
            
        } else if (isSelecting && selectionStart !== null) {
            selectionEnd = clampedX;
            drawTimeline();

            const rect = tl.getBoundingClientRect();
            const pad = 0;
            const rightEdge = rect.width - pad;

            if (Math.abs(clampedX - rightEdge) < 5) {
                showNotification('Release to switch to live mode', 1000);
            }
        }
    });

    window.addEventListener('mouseup', () => {
        if (isSelecting && selectionStart !== null && selectionEnd !== null) {
            const rect = tl.getBoundingClientRect();
            const pad = 0;

            const xStart = Math.min(selectionStart, selectionEnd);
            const xEnd = Math.max(selectionStart, selectionEnd);

            const rightEdge = rect.width - pad;
            const reachedLive = Math.abs(xEnd - rightEdge) < 10;

            if (reachedLive) {
                followLive = true;
                pausedEndTs = null;
                windowStartMs = null;

                const winSlider = document.getElementById('win-slider');
                const winLabel = document.getElementById('win-slider-label');
                const endSlider = document.getElementById('end-slider');
                const endLabel = document.getElementById('end-slider-label');

                windowMs = 180000;
                winSlider.value = '3';
                winLabel.textContent = '3m';

                endSlider.value = endSlider.max;
                endLabel.textContent = 'live';

                document.querySelectorAll('button[data-win]').forEach(b => b.classList.remove('active'));
                document.querySelector('button[data-win="180000"]')?.classList.add('active');

                drawAllCharts();
                showNotification('Switched to live mode');
            } else if (xEnd - xStart > 5) {
                const minX = data.xs[0];
                const maxX = data.xs[data.xs.length - 1];

                const timeStart = minX + ((xStart - pad) / (rect.width - 2 * pad)) * (maxX - minX);
                const timeEnd = minX + ((xEnd - pad) / (rect.width - 2 * pad)) * (maxX - minX);

                windowMs = timeEnd - timeStart;
                windowStartMs = timeStart;
                pausedEndTs = timeEnd;
                followLive = false;

                const winSlider = document.getElementById('win-slider');
                const winLabel = document.getElementById('win-slider-label');
                
                if (windowMs >= 60000) {
                    const mins = Math.round(windowMs / 60000);
                    winSlider.value = Math.min(60, Math.max(1, mins));
                    winLabel.textContent = (mins >= 60 ? '60' : mins) + 'm';
                } else {
                    winSlider.value = '1';
                    winLabel.textContent = '1m';
                }

                document.querySelectorAll('button[data-win]').forEach(b => b.classList.remove('active'));

                updateEndSlider();
                drawAllCharts();

                const durationSec = Math.round(windowMs / 1000);
                const durationMin = Math.floor(durationSec / 60);
                const durationRemSec = durationSec % 60;
                
                if (durationMin > 0) {
                    showNotification(`Selected range: ${durationMin}m ${durationRemSec}s (${fmtTime(timeStart)} - ${fmtTime(timeEnd)})`);
                } else {
                    showNotification(`Selected range: ${durationSec}s (${fmtTime(timeStart)} - ${fmtTime(timeEnd)})`);
                }
            } else {
                if (windowMs > 0 && windowStartMs) {
                    const minX = data.xs[0];
                    const maxX = data.xs[data.xs.length - 1];
                    const clickTime = minX + ((xStart - pad) / (rect.width - 2 * pad)) * (maxX - minX);
                    
                    windowStartMs = clamp(clickTime - windowMs/2, minX, maxX - windowMs);
                    pausedEndTs = windowStartMs + windowMs;
                    followLive = false;
                    
                    updateEndSlider();
                    drawAllCharts();
                }
            }
        }

        isDraggingWindow = false;
        isSelecting = false;
        isResizingLeft = false;
        isResizingRight = false;
        isMoving = false;
        selectionStart = null;
        selectionEnd = null;
        tl.style.cursor = 'default';
        drawTimeline();
    });

    tl.addEventListener('dblclick', () => {
        followLive = true;
        pausedEndTs = null;
        windowStartMs = null;
        
        const winSlider = document.getElementById('win-slider');
        const winLabel = document.getElementById('win-slider-label');
        winSlider.value = '3';
        winLabel.textContent = '3m';
        windowMs = 180000;
        
        document.querySelectorAll('button[data-win]').forEach(b => b.classList.remove('active'));
        document.querySelector('button[data-win="180000"]')?.classList.add('active');
        
        updateEndSlider();
        drawAllCharts();
        
        showNotification('Returned to live mode');
    });
}

function showNotification(message, duration = 3000) {
    let notification = document.getElementById('timeline-notification');
    if (!notification) {
        notification = document.createElement('div');
        notification.id = 'timeline-notification';
        notification.style.cssText = `
            position: fixed;
            bottom: 24px;
            left: 50%;
            transform: translateX(-50%);
            background: #1b2a4a;
            color: #e5e7eb;
            padding: 10px 20px;
            border-radius: 24px;
            font-size: 14px;
            font-family: ui-monospace, monospace;
            border: 1px solid #3b82f6;
            box-shadow: 0 4px 12px rgba(0,0,0,0.5);
            z-index: 2000;
            transition: opacity 0.3s;
            white-space: nowrap;
            max-width: 90%;
            overflow: hidden;
            text-overflow: ellipsis;
        `;
        document.body.appendChild(notification);
    }
    
    notification.textContent = message;
    notification.style.opacity = '1';
    
    if (window.notificationTimeout) {
        clearTimeout(window.notificationTimeout);
    }
    
    window.notificationTimeout = setTimeout(() => {
        notification.style.opacity = '0';
    }, duration);
}

function updateRangeLabel(view) {
    const label = document.getElementById('range-label');
    const duration = (view.endTs - view.startTs) / 1000;
    const hours = Math.floor(duration / 3600);
    const minutes = Math.floor((duration % 3600) / 60);
    const seconds = Math.floor(duration % 60);
    
    let durationStr = '';
    if (hours > 0) {
        durationStr = `${hours}h ${minutes}m ${seconds}s`;
    } else if (minutes > 0) {
        durationStr = `${minutes}m ${seconds}s`;
    } else {
        durationStr = `${seconds}s`;
    }
    
    if (windowMs === 0) {
        label.textContent = `All data: ${fmtTime(view.startTs)} - ${fmtTime(view.endTs)} (${durationStr})`;
    } else {
        label.textContent = `${fmtTime(view.startTs)} - ${fmtTime(view.endTs)} (${durationStr})`;
    }
}

async function fetchHistory() {
    try {
        const res = await fetch('/api/history?limit=10000');
        if (!res.ok) throw new Error('HTTP ' + res.status);

        const hist = await res.json();
        resetData();

        if (Array.isArray(hist) && hist.length > 0) {
            hist.forEach(p => pushDataPoint(p));
            if (hist.length > 0) {
                createChartsFromSnapshot(hist[hist.length - 1]);
            }
        }

        drawAllCharts();
    } catch (e) {
        console.error('Fetch error:', e);
    }
}

function startStream() {
    const es = new EventSource('/api/stream');

    es.onmessage = (ev) => {
        try {
            const p = JSON.parse(ev.data);
            document.getElementById('latest').textContent = JSON.stringify(p, null, 2);
            
            if (data.xs.length === 0) {
                createChartsFromSnapshot(p);
            }

            pushDataPoint(p);
            updateStatCards(p);

            if (followLive) {
                drawAllCharts();
                if (fullscreenName) drawFullscreenChart();
            }
        } catch (e) {
            console.error('Stream error:', e);
        }
    };
}

function initWindowButtons() {
    const buttons = document.querySelectorAll('button[data-win]');

    buttons.forEach(btn => {
        btn.addEventListener('click', () => {
            const ms = parseInt(btn.dataset.win, 10);

            buttons.forEach(b => b.classList.remove('active'));
            btn.classList.add('active');

            windowMs = ms;
            followLive = true;
            pausedEndTs = null;

            const winSlider = document.getElementById('win-slider');
            const winLabel = document.getElementById('win-slider-label');

            if (ms === 0) {
                winSlider.value = '60';
                winLabel.textContent = 'All';
            } else {
                const mins = Math.round(ms / 60000);
                winSlider.value = mins;
                winLabel.textContent = mins + 'm';
            }

            drawAllCharts();
        });
    });
}

function initSliders() {
    const winSlider = document.getElementById('win-slider');
    const winLabel = document.getElementById('win-slider-label');
    const endSlider = document.getElementById('end-slider');
    const endLabel = document.getElementById('end-slider-label');
    const liveBtn = document.getElementById('live-btn');

    winSlider.addEventListener('input', () => {
        const mins = parseInt(winSlider.value, 10);
        windowMs = mins * 60000;
        winLabel.textContent = mins + 'm';

        document.querySelectorAll('button[data-win]').forEach(b => b.classList.remove('active'));
    });

    winSlider.addEventListener('change', () => {
        drawAllCharts();
    });

    endSlider.addEventListener('input', () => {
        if (data.xs.length < 2) return;

        const val = parseInt(endSlider.value, 10);
        const max = parseInt(endSlider.max, 10);

        followLive = (val === max);

        if (!followLive && data.xs.length > 0) {
            const startTs = data.xs[0];
            pausedEndTs = startTs + val * 1000;
        } else {
            pausedEndTs = null;
        }

        updateEndSlider();
        drawAllCharts();
    });

    liveBtn.addEventListener('click', () => {
        followLive = true;
        pausedEndTs = null;

        if (data.xs.length > 0) {
            const endSlider = document.getElementById('end-slider');
            endSlider.value = endSlider.max;
            document.getElementById('end-slider-label').textContent = 'live';
        }

        drawAllCharts();
    });
}

function updateEndSlider() {
    const endSlider = document.getElementById('end-slider');
    const endLabel = document.getElementById('end-slider-label');

    if (data.xs.length < 2) return;

    const startTs = data.xs[0];
    const endTs = data.xs[data.xs.length - 1];
    const totalSeconds = Math.floor((endTs - startTs) / 1000);

    endSlider.max = totalSeconds;

    if (followLive) {
        endSlider.value = totalSeconds;
        endLabel.textContent = 'live';
    } else if (pausedEndTs) {
        const seconds = Math.floor((pausedEndTs - startTs) / 1000);
        endSlider.value = clamp(seconds, 0, totalSeconds);

        const behindSeconds = Math.floor((endTs - pausedEndTs) / 1000);

        if (behindSeconds < 60) {
            endLabel.textContent = `${behindSeconds}s behind`;
        } else {
            const behindMinutes = Math.floor(behindSeconds / 60);
            const remainingSeconds = behindSeconds % 60;
            if (remainingSeconds === 0) {
                endLabel.textContent = `${behindMinutes}m behind`;
            } else {
                endLabel.textContent = `${behindMinutes}m ${remainingSeconds}s behind`;
            }
        }
    }
}

function initWidgetMenu() {
    const btn = document.getElementById('widget-menu-btn');
    const menu = document.getElementById('widget-menu');
    if (!btn || !menu) return;

    btn.addEventListener('click', (e) => {
        e.stopPropagation();
        menu.classList.toggle('open');
    });
    document.addEventListener('click', (e) => {
        if (!menu.contains(e.target) && e.target !== btn) {
            menu.classList.remove('open');
        }
    });
}

document.addEventListener('DOMContentLoaded', () => {
    initWindowButtons();
    initSliders();
    initWidgetMenu();
    fetchHistory();
    startStream();
    setupTimelineDrag();
});