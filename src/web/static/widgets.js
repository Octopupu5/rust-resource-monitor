let data = { xs: [], series: {} };
let followLive = true;
let windowMs = 180000;
let pausedEndTs = null;
let windowStartMs = null;
const tooltip = document.getElementById('tooltip');
let lastView = null;
let hiddenSeries = {};

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

function createChartsFromSnapshot(snapshot) {
    const container = document.getElementById('charts-container');
    if (!container) return;

    container.innerHTML = '';

    snapshot.data.forEach(series => {
        if (series.series.length === 0) return;

        const panel = document.createElement('div');
        panel.className = 'panel';
        panel.setAttribute('data-series-name', series.name);

        const title = document.createElement('h3');
        title.textContent = series.beautiful_name || series.name;
        panel.appendChild(title);

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
        container.appendChild(panel);
    });

    setupChartHandlers();
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
                legend: series.legend,
                format: series.format,
                beautiful_name: series.beautiful_name
            };
        }
        data.series[series.name].values.push(series.series);
    });

    const maxLen = 20000;
    if (data.xs.length > maxLen) {
        const drop = data.xs.length - maxLen;
        data.xs.splice(0, drop);
        Object.keys(data.series).forEach(name => {
            data.series[name].values.splice(0, drop);
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

        for (let i = 0; i < seriesData.legend.length; i++) {
            if (hiddenSeries[name]?.[i]) continue;
            
            const legend = seriesData.legend[i];
            const ys = [];
            for (let j = view.startIdx; j < view.endIdx; j++) {
                ys.push(seriesData.values[j]?.[i] || 0);
            }
            seriesList.push({
                ys: ys,
                color: legend.color,
                lineWidth: seriesData.legend.length > 8 ? 1 : 2
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
            byteY: seriesData.format?.type === 'Bytes'
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
    const rightPad = 10;
    const topPad = 10;
    const bottomPad = 20;

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
        bottomPad 
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
    const pad = 70;
    const timeLabelArea = 35;

    if (data.series['cpu_total'] && data.series['cpu_total'].values.length > 0) {
        ctx.strokeStyle = 'rgba(196, 68, 68, 0.5)';
        ctx.lineWidth = 1;
        ctx.beginPath();

        const step = Math.max(1, Math.floor(data.xs.length / 200));
        let first = true;

        for (let i = 0; i < data.xs.length; i += step) {
            const x = pad + ((data.xs[i] - minX) / (maxX - minX)) * (w - 2 * pad);
            const y = 10 + ((data.series['cpu_total'].values[i][0] || 0) / 100) * (h - pad - 20);

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
    ctx.textAlign = 'center';
    
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
        ctx.fillText(timeStr, x, h - timeLabelArea + 8);
    }

    if (windowMs > 0 && windowStartMs) {
        const x0 = pad + ((windowStartMs - minX) / (maxX - minX)) * (w - 2 * pad);
        const x1 = pad + ((windowStartMs + windowMs - minX) / (maxX - minX)) * (w - 2 * pad);

        const clampedX0 = Math.max(pad, Math.min(w - pad, x0));
        const clampedX1 = Math.max(pad, Math.min(w - pad, x1));
        ctx.font = '9px ui-monospace, monospace';
        ctx.fillStyle = '#3b82f6';
        const startTimeStr = fmtTime(windowStartMs);
        const endTimeStr = fmtTime(windowStartMs + windowMs);

        const textWidth = ctx.measureText(startTimeStr).width;
        if (clampedX0 + textWidth < clampedX1) {
            ctx.fillText(startTimeStr, clampedX0, h - timeLabelArea + 20);
            ctx.fillText(endTimeStr, clampedX1 - 40, h - timeLabelArea + 20);
        }
    }

    if (selectionStart !== null && selectionEnd !== null && selectionEnd - selectionStart > 20) {
        const xStart = Math.max(pad, Math.min(w - pad, Math.min(selectionStart, selectionEnd)));
        const xEnd = Math.max(pad, Math.min(w - pad, Math.max(selectionStart, selectionEnd)));
        const minX = data.xs[0];
        const maxX = data.xs[data.xs.length - 1];

        const timeStart = minX + ((xStart - pad) / (w - 2 * pad)) * (maxX - minX);
        const timeEnd = minX + ((xEnd - pad) / (w - 2 * pad)) * (maxX - minX);

        ctx.font = '9px ui-monospace, monospace';
        ctx.fillStyle = '#ec4899';

        const startTimeStr = fmtTime(timeStart);
        const endTimeStr = fmtTime(timeEnd);

        const textWidth = ctx.measureText(startTimeStr).width;
        if (xStart + textWidth < xEnd) {
            ctx.fillText(startTimeStr, xStart, h - timeLabelArea + 20);
            ctx.fillText(endTimeStr, xEnd - 40, h - timeLabelArea + 20);
        }
    }

    ctx.font = '10px ui-monospace, monospace';
    ctx.textAlign = 'left';
    ctx.textBaseline = 'top';

    ctx.fillStyle = '#3b82f6';
    ctx.fillRect(10, 10, 12, 12);
    ctx.fillStyle = '#9ca3af';
    ctx.fillText('Current view', 28, 10);

    ctx.fillStyle = '#ec4899';
    ctx.fillRect(10, 30, 12, 12);
    ctx.fillStyle = '#9ca3af';
    ctx.fillText('Selection', 28, 30);

    ctx.textAlign = 'right';
    ctx.fillStyle = '#6b7280';
    ctx.font = '9px ui-monospace, monospace';
    ctx.fillText('Drag inside to move', w - 10, 10);
    ctx.fillText('Drag edges to resize', w - 10, 25);
    ctx.fillText('Drag outside to select', w - 10, 40);
    ctx.fillText('Double-click → live', w - 10, 55);
    ctx.fillText('Drag to end → auto-live', w - 10, 70);
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
                const pad = 40;

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

            const rows = [`<div style="color:#9ca3af;">${fmtTime(xs[nearestIdx])}</div>`];

            for (let j = 0; j < seriesData.legend.length; j++) {
                if (hiddenSeries[seriesName]?.[j]) continue;
                
                const legend = seriesData.legend[j];
                const v = seriesData.values[nearestIdx + lastView.startIdx]?.[j] || 0;

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
                
                rows.push(`<div><span style="color:${legend.color};">${legend.name}</span>: ${formatValue(v, seriesData.format)}</div>`);
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
        const pad = 70;
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
                const pad = 40;
                
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
        const pad = 40;
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
            const pad = 40;
            const rightEdge = rect.width - pad;

            if (Math.abs(clampedX - rightEdge) < 5) {
                showNotification('Release to switch to live mode', 1000);
            }
        }
    });

    window.addEventListener('mouseup', () => {
        if (isSelecting && selectionStart !== null && selectionEnd !== null) {
            const rect = tl.getBoundingClientRect();
            const pad = 40;

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

document.addEventListener('DOMContentLoaded', () => {
    initWindowButtons();
    initSliders();
    fetchHistory();
    startStream();
    setupTimelineDrag();
});