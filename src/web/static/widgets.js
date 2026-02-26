function clamp(x, lo, hi) {
      if (!Number.isFinite(x)) return lo;
      return Math.max(lo, Math.min(hi, x));
    }

function fmtBytes(v, suffix) {
    suffix = suffix || 'B/s';
    if (!Number.isFinite(v) || v < 0) return '0 ' + suffix;
    if (v >= 1073741824) return (v / 1073741824).toFixed(2) + ' Gi' + suffix;
    if (v >= 1048576)    return (v / 1048576).toFixed(2)    + ' Mi' + suffix;
    if (v >= 1024)       return (v / 1024).toFixed(1)       + ' Ki' + suffix;
    return v.toFixed(0) + ' ' + suffix;
}

function fmtBytesAuto(v, maxY) {
    if (maxY >= 1073741824) return (v / 1073741824).toFixed(2) + ' GiB/s';
    if (maxY >= 1048576)    return (v / 1048576).toFixed(2)    + ' MiB/s';
    if (maxY >= 1024)       return (v / 1024).toFixed(1)       + ' KiB/s';
    return v.toFixed(0) + ' B/s';
}

function byteScale(maxY) {
    if (maxY >= 1073741824) return { div: 1073741824, unit: 'GiB/s' };
    if (maxY >= 1048576)    return { div: 1048576,    unit: 'MiB/s' };
    if (maxY >= 1024)       return { div: 1024,       unit: 'KiB/s' };
    return { div: 1, unit: 'B/s' };
}

function drawLineChart(canvas, series, options) {
    const ctx = canvas.getContext('2d');
    const w = canvas.width, h = canvas.height;
    ctx.clearRect(0, 0, w, h);

    // Background
    ctx.fillStyle = '#0f1626';
    ctx.fillRect(0, 0, w, h);

    // Grid
    ctx.strokeStyle = '#1c2740';
    ctx.lineWidth = 1;
    for (let i = 0; i <= 10; i++) {
        const y = (h * i) / 10;
        ctx.beginPath();
        ctx.moveTo(0, y);
        ctx.lineTo(w, y);
        ctx.stroke();
    }

    const xs = options.xs;
    const minX = Math.min(...xs);
    const maxX = Math.max(...xs);
    const minY = options.minY;
    const maxY = options.maxY;

    const leftPad = 64;
    const rightPad = 10;
    const topPad = 10;
    const bottomPad = 24;

    // Save metadata for mouse drag selection (zoom).
    canvas.__meta = { minX, maxX, minY, maxY, w, h, leftPad, rightPad, topPad, bottomPad };

    function xToPx(x) {
        if (maxX === minX) return leftPad;
        return (x - minX) / (maxX - minX) * (w - leftPad - rightPad) + leftPad;
    }
    function yToPx(y) {
        const t = (y - minY) / (maxY - minY);
        return (1 - clamp(t, 0, 1)) * (h - topPad - bottomPad) + topPad;
    }

    ctx.fillStyle = '#9ca3af';
    ctx.font = '12px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace';
    ctx.textAlign = 'left';
    ctx.textBaseline = 'middle';

    const yTicks = 5;
    const scale = options.byteY ? byteScale(maxY) : null;
    for (let i = 0; i < yTicks; i++) {
        const v = minY + (maxY - minY) * (i / (yTicks - 1));
        const py = yToPx(v);
        let label;
        if (scale) {
            label = (v / scale.div).toFixed(v / scale.div >= 10 ? 0 : 1) + ' ' + scale.unit;
        } else {
            const dec = (Math.abs(maxY - minY) >= 20 || maxY >= 20) ? 0 : 1;
            label = v.toFixed(dec);
        }
        ctx.fillText(label, 4, py);
    }

    if (Number.isFinite(minX) && Number.isFinite(maxX)) {
        ctx.textBaseline = 'alphabetic';
        const xTicks = Math.min(7, Math.max(3, Math.floor((w - leftPad - rightPad) / 120) + 1));
        for (let i = 0; i < xTicks; i++) {
            const ts = minX + (maxX - minX) * (i / (xTicks - 1));
            const px = xToPx(ts);
            const txt = new Date(ts).toLocaleTimeString();
            const tw = ctx.measureText(txt).width;
            const x = clamp(px - tw / 2, leftPad, w - rightPad - tw);
            ctx.fillText(txt, x, h - 6);
        }
    }

    for (const s of series) {
        ctx.strokeStyle = s.color;
        ctx.lineWidth = s.lineWidth || 2;
        ctx.beginPath();
        for (let i = 0; i < xs.length; i++) {
            const px = xToPx(xs[i]);
            const py = yToPx(s.ys[i]);
            if (i === 0) ctx.moveTo(px, py);
            else ctx.lineTo(px, py);
        }
        ctx.stroke();
    }
}

function windowLabel(ms) {
    if (ms === 0) return 'All (buffer)';
    const min = Math.round(ms / 60000);
    return `Last ${min} minute${min === 1 ? '' : 's'}`;
}

let windowMs = 180000; // Default: 3 minutes
let followLive = true;

let pausedEndTs = null;

let data = { xs: [], cpu: [], mem: [], rx: [], tx: [], la1: [], la5: [], la15: [], disk: [], swap: [], cores: [] };
const tooltip = document.getElementById('tooltip');
const overlays = {
    cpu:   document.getElementById('cpu-ov'),
    mem:   document.getElementById('mem-ov'),
    net:   document.getElementById('net-ov'),
    load:  document.getElementById('load-ov'),
    cores: document.getElementById('cores-ov'),
    disk:  document.getElementById('disk-ov'),
    swap:  document.getElementById('swap-ov'),
};
let lastView = null;
let netMaxY = 1;

function resetData() {
    data = { xs: [], cpu: [], mem: [], rx: [], tx: [], la1: [], la5: [], la15: [], disk: [], swap: [], cores: [] };
    netMaxY = 1;
}

function pushDataPoint(p) {
    const ts = p.timestamp_ms;
    if (typeof ts !== 'number') return;
    const last = data.xs.length ? data.xs[data.xs.length - 1] : 0;
    if (ts <= last) return;

    data.xs.push(ts);
    data.cpu.push(p.cpu.total_usage_pct);

    const total = (p.memory.total_bytes || 0);
    const used  = (p.memory.used_bytes  || 0);
    data.mem.push(total === 0 ? 0 : used / total * 100);

    const swapTotal = (p.memory.swap_total_bytes || 0);
    const swapUsed  = (p.memory.swap_used_bytes  || 0);
    data.swap.push(swapTotal === 0 ? 0 : swapUsed / swapTotal * 100);

    data.rx.push(p.network.rx_bytes_per_sec);
    data.tx.push(p.network.tx_bytes_per_sec);

    data.la1.push(p.cpu.load_avg_1);
    data.la5.push(p.cpu.load_avg_5);
    data.la15.push(p.cpu.load_avg_15);

    data.disk.push((p.disk && p.disk.used_pct) || 0);

    data.cores.push(Array.isArray(p.cpu.per_core_usage_pct) ? p.cpu.per_core_usage_pct : []);

    const maxLen = 20000;
    if (data.xs.length > maxLen) {
        const drop = data.xs.length - maxLen;
        for (const k of ['xs','cpu','mem','rx','tx','la1','la5','la15','disk','swap','cores']) data[k].splice(0, drop);
    }
    updateEndSliderMax();
}

function lowerBound(arr, x) {
    let lo = 0, hi = arr.length;
    while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (arr[mid] < x) lo = mid + 1;
        else hi = mid;
    }
    return lo;
}

function currentViewRange() {
    if (data.xs.length === 0) return null;
    const startTs = data.xs[0];
    const endTs   = data.xs[data.xs.length - 1];

    const viewEnd     = followLive ? endTs : (pausedEndTs ?? endTs);
    const clampedEnd  = clamp(viewEnd, startTs, endTs);
    const viewStart   = windowMs === 0 ? startTs : Math.max(startTs, clampedEnd - windowMs);
    return { startTs, endTs, viewStart, viewEnd: clampedEnd };
}

function viewSeries() {
    const r = currentViewRange();
    if (!r) return null;
    const i0 = lowerBound(data.xs, r.viewStart);
    const i1 = lowerBound(data.xs, r.viewEnd + 1);

    const coreCount = data.cores.length > 0
        ? (data.cores[Math.min(i0, data.cores.length - 1)] || []).length
        : 0;
    const coreLines = [];
    for (let c = 0; c < coreCount; c++) {
        const ys = [];
        for (let t = i0; t < i1; t++) {
            ys.push((data.cores[t] || [])[c] ?? 0);
        }
        coreLines.push(ys);
    }

    return {
        xs:       data.xs.slice(i0, i1),
        cpu:      data.cpu.slice(i0, i1),
        mem:      data.mem.slice(i0, i1),
        rx:       data.rx.slice(i0, i1),
        tx:       data.tx.slice(i0, i1),
        la1:      data.la1.slice(i0, i1),
        la5:      data.la5.slice(i0, i1),
        la15:     data.la15.slice(i0, i1),
        disk:     data.disk.slice(i0, i1),
        swap:     data.swap.slice(i0, i1),
        coreLines,
        range: r,
    };
}

function coreColor(c, n) {
    const hue = Math.round((c / Math.max(n, 1)) * 360);
    return `hsl(${hue}, 80%, 60%)`;
}

function redraw() {
    const s = viewSeries();
    if (!s || s.xs.length === 0) return;
    lastView = s;

    for (const ov of Object.values(overlays)) {
        const ctx = ov.getContext('2d');
        ctx.clearRect(0, 0, ov.width, ov.height);
    }
    tooltip.style.display = 'none';

    // --- CPU total ---
    drawLineChart(document.getElementById('cpu'), [{ ys: s.cpu, color: '#c44' }], {
        xs: s.xs, minY: 0, maxY: 100,
    });

    // --- Memory used % ---
    drawLineChart(document.getElementById('mem'), [{ ys: s.mem, color: '#f59e0b' }], {
        xs: s.xs, minY: 0, maxY: 100,
    });

    // --- Network (auto-scaled bytes/s) ---
    const maxNetView = Math.max(1, ...s.rx, ...s.tx);
    netMaxY = Math.max(netMaxY, maxNetView);
    drawLineChart(document.getElementById('net'), [
        { ys: s.rx, color: '#0b6' },
        { ys: s.tx, color: '#06b' },
    ], {
        xs: s.xs, minY: 0, maxY: netMaxY * 1.1, byteY: true,
    });

    // --- CPU Load Average ---
    const maxLA = Math.max(0.1, ...s.la1, ...s.la5, ...s.la15);
    drawLineChart(document.getElementById('load'), [
        { ys: s.la1,  color: '#e879f9' },
        { ys: s.la5,  color: '#a78bfa' },
        { ys: s.la15, color: '#60a5fa' },
    ], {
        xs: s.xs, minY: 0, maxY: maxLA * 1.15,
    });

    // --- Per-core CPU ---
    const n = s.coreLines.length;
    const coreSeries = s.coreLines.map((ys, c) => ({
        ys,
        color: coreColor(c, n),
        lineWidth: n > 8 ? 1 : 2,
    }));
    drawLineChart(document.getElementById('cores'), coreSeries, {
        xs: s.xs, minY: 0, maxY: 100,
    });
    // Update core legend
    const legend = document.getElementById('cores-legend');
    if (legend) {
        legend.innerHTML = s.coreLines.slice(0, 32).map((_, c) =>
            `<span style="color:${coreColor(c, n)};">C${c}</span>`
        ).join(' ');
    }

    // --- Disk space used % ---
    drawLineChart(document.getElementById('disk'), [{ ys: s.disk, color: '#34d399' }], {
        xs: s.xs, minY: 0, maxY: 100,
    });

    // --- Swap used % ---
    drawLineChart(document.getElementById('swap'), [{ ys: s.swap, color: '#818cf8' }], {
        xs: s.xs, minY: 0, maxY: 100,
    });

    updateRangeLabel(s.range, s.xs.length);
    drawTimeline();
}

function updateStatCards(p) {
    const total = p.memory.total_bytes || 0;
    const used  = p.memory.used_bytes  || 0;
    const memPct = total === 0 ? 0 : (used / total * 100);
    const swapTotal = p.memory.swap_total_bytes || 0;
    const swapUsed  = p.memory.swap_used_bytes  || 0;
    const swapPct   = swapTotal === 0 ? 0 : (swapUsed / swapTotal * 100);

    const set = (id, v) => { const el = document.getElementById(id); if (el) el.textContent = v; };
    set('sc-cpu',  p.cpu.total_usage_pct.toFixed(1) + '%');
    set('sc-mem',  memPct.toFixed(1) + '%');
    set('sc-la1',  p.cpu.load_avg_1.toFixed(2));
    set('sc-la5',  p.cpu.load_avg_5.toFixed(2));
    set('sc-la15', p.cpu.load_avg_15.toFixed(2));
    set('sc-rx',   fmtBytes(p.network.rx_bytes_per_sec));
    set('sc-tx',   fmtBytes(p.network.tx_bytes_per_sec));
    set('sc-disk', ((p.disk && p.disk.used_pct) || 0).toFixed(1) + '%');
    set('sc-swap', swapTotal === 0 ? 'N/A' : swapPct.toFixed(1) + '%');
}

function fmtTime(ms) {
    const d = new Date(ms);
    return d.toLocaleTimeString();
}

function updateRangeLabel(r, points) {
    const label = document.getElementById('range-label');
    if (windowMs === 0) {
        label.textContent = `All (buffer): ${fmtTime(r.startTs)} - ${fmtTime(r.endTs)} | points=${points}`;
    } else {
        label.textContent = `${fmtTime(r.viewStart)} - ${fmtTime(r.viewEnd)} | window=${Math.round(windowMs/60000)}m | points=${points}`;
    }
}

function updateEndSliderMax() {
    const endSlider = document.getElementById('end-slider');
    const endLabel  = document.getElementById('end-slider-label');
    if (data.xs.length < 2) {
        endSlider.max   = 0;
        endSlider.value = 0;
        endLabel.textContent = 'live';
        return;
    }
    const startTs = data.xs[0];
    const endTs   = data.xs[data.xs.length - 1];
    const spanSec = Math.max(0, Math.floor((endTs - startTs) / 1000));
    endSlider.max = String(spanSec);
    if (followLive) {
        endSlider.value = String(spanSec);
    } else {
        const targetEnd = pausedEndTs ?? endTs;
        const endSec    = clamp(Math.floor((targetEnd - startTs) / 1000), 0, spanSec);
        endSlider.value = String(endSec);
    }
    endLabel.textContent = followLive ? 'live' : `paused (t-${spanSec - Number(endSlider.value || 0)}s)`;
}

function tsFromCanvasX(canvas, xPx) {
    const m = canvas.__meta;
    if (!m) return null;
    const { minX, maxX, w } = m;
    const leftPad  = m.leftPad  ?? m.pad;
    const rightPad = m.rightPad ?? m.pad;
    if (maxX === minX) return null;
    const x = clamp(xPx, leftPad, w - rightPad);
    const t = (x - leftPad) / (w - leftPad - rightPad);
    return minX + t * (maxX - minX);
}

function xToPxFromMeta(m, x) {
    if (!m || m.maxX === m.minX) return m ? (m.leftPad ?? m.pad) : 0;
    const leftPad  = m.leftPad  ?? m.pad;
    const rightPad = m.rightPad ?? m.pad;
    return (x - m.minX) / (m.maxX - m.minX) * (m.w - leftPad - rightPad) + leftPad;
}

function yToPxFromMeta(m, y) {
    const topPad    = m.topPad    ?? m.pad;
    const bottomPad = m.bottomPad ?? m.pad;
    const t = (y - m.minY) / (m.maxY - m.minY);
    return (1 - clamp(t, 0, 1)) * (m.h - topPad - bottomPad) + topPad;
}

function resizeCanvasToDisplaySize(canvas) {
    const dpr  = window.devicePixelRatio || 1;
    const rect = canvas.getBoundingClientRect();
    const w    = Math.max(1, Math.floor(rect.width  * dpr));
    const h    = Math.max(1, Math.floor(rect.height * dpr));
    if (canvas.width !== w || canvas.height !== h) {
        canvas.width  = w;
        canvas.height = h;
    }
    return { dpr, rect };
}

function drawTimeline() {
    const tl = document.getElementById('timeline');
    if (!tl) return;
    const { dpr } = resizeCanvasToDisplaySize(tl);
    const ctx = tl.getContext('2d');
    const w = tl.width, h = tl.height;
    ctx.clearRect(0, 0, w, h);

    // Background.
    ctx.fillStyle = '#0f1626';
    ctx.fillRect(0, 0, w, h);

    if (data.xs.length < 2) {
        document.getElementById('brush-label').textContent = 'Waiting for data...';
        return;
    }

    const minX = data.xs[0];
    const maxX = data.xs[data.xs.length - 1];
    const pad  = Math.round(6 * dpr);
    const minY = 0;
    const maxY = 100;
    tl.__meta  = { minX, maxX, minY, maxY, w, h, pad };

    // Grid.
    ctx.strokeStyle = '#1c2740';
    ctx.lineWidth   = 1;
    for (let i = 0; i <= 4; i++) {
        const y = (h * i) / 4;
        ctx.beginPath();
        ctx.moveTo(0, y);
        ctx.lineTo(w, y);
        ctx.stroke();
    }

    ctx.strokeStyle = 'rgba(196, 68, 68, 0.8)';
    ctx.lineWidth   = 1;
    const n    = data.xs.length;
    const step = Math.max(1, Math.floor(n / 600));
    ctx.beginPath();
    for (let i = 0; i < n; i += step) {
        const px = xToPxFromMeta(tl.__meta, data.xs[i]);
        const py = yToPxFromMeta(tl.__meta, data.cpu[i] || 0);
        if (i === 0) ctx.moveTo(px, py);
        else ctx.lineTo(px, py);
    }
    ctx.stroke();

    const r = currentViewRange();
    if (!r) return;
    const x0 = xToPxFromMeta(tl.__meta, r.viewStart);
    const x1 = xToPxFromMeta(tl.__meta, r.viewEnd);

    // Selection brush.
    ctx.save();
    ctx.fillStyle   = 'rgba(59, 130, 246, 0.20)';
    ctx.strokeStyle = 'rgba(59, 130, 246, 0.85)';
    ctx.lineWidth   = Math.max(1, Math.round(1 * dpr));
    ctx.fillRect(x0, 0, Math.max(1, x1 - x0), h);
    ctx.beginPath();
    ctx.moveTo(x0, 0); ctx.lineTo(x0, h);
    ctx.moveTo(x1, 0); ctx.lineTo(x1, h);
    ctx.stroke();
    // Handles.
    ctx.fillStyle = 'rgba(59, 130, 246, 0.85)';
    ctx.fillRect(x0 - 2 * dpr, 0, 4 * dpr, h);
    ctx.fillRect(x1 - 2 * dpr, 0, 4 * dpr, h);
    ctx.restore();

    // Time tick marks (bottom).
    ctx.save();
    ctx.fillStyle = '#9ca3af';
    ctx.font = `${Math.max(10, Math.floor(11 * dpr))}px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace`;
    const ticks = 8;
    for (let i = 0; i < ticks; i++) {
        const ts  = minX + (maxX - minX) * (i / (ticks - 1));
        const x   = xToPxFromMeta(tl.__meta, ts);
        ctx.strokeStyle = 'rgba(28, 39, 64, 0.8)';
        ctx.beginPath();
        ctx.moveTo(x, h - Math.round(14 * dpr));
        ctx.lineTo(x, h);
        ctx.stroke();
        const txt = fmtTime(ts);
        const tw  = ctx.measureText(txt).width;
        ctx.fillText(txt, clamp(x - tw / 2, pad, w - pad - tw), h - Math.round(2 * dpr));
    }
    ctx.restore();

    // Selection boundary labels.
    ctx.save();
    ctx.fillStyle = '#e5e7eb';
    ctx.font = `${Math.max(10, Math.floor(11 * dpr))}px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace`;
    ctx.fillText(fmtTime(r.viewStart), clamp(x0 + 4 * dpr, pad, w - pad), Math.round(14 * dpr));
    const endTxt = fmtTime(r.viewEnd);
    const endTw  = ctx.measureText(endTxt).width;
    ctx.fillText(endTxt, clamp(x1 - endTw - 4 * dpr, pad, w - pad - endTw), Math.round(28 * dpr));
    ctx.restore();

    const isLive = followLive;
    document.getElementById('brush-label').textContent =
        `${fmtTime(r.viewStart)} - ${fmtTime(r.viewEnd)}${isLive ? ' (live)' : ''}`;
}

function installTimelineBrush() {
    const tl = document.getElementById('timeline');
    if (!tl) return;
    let dragging   = false;
    let mode       = 'new'; // 'new' | 'move'
    let startX     = 0;
    let curX       = 0;
    let moveOffset = 0;
    let selWidth   = 0;

    function pxFromEvent(e) {
        const rect = tl.getBoundingClientRect();
        const dpr  = window.devicePixelRatio || 1;
        return (e.clientX - rect.left) * dpr;
    }

    function selectionPx() {
        const r = currentViewRange();
        const m = tl.__meta;
        if (!r || !m) return null;
        const x0 = xToPxFromMeta(m, r.viewStart);
        const x1 = xToPxFromMeta(m, r.viewEnd);
        return { x0, x1 };
    }

    function redrawWithOverlay() {
        drawTimeline();
        if (!dragging) return;
        const ctx = tl.getContext('2d');
        const x0 = mode === 'move' ? (curX - moveOffset) : Math.min(startX, curX);
        const x1 = mode === 'move' ? (x0 + selWidth)     : Math.max(startX, curX);
        ctx.save();
        ctx.fillStyle   = 'rgba(147, 197, 253, 0.10)';
        ctx.strokeStyle = 'rgba(147, 197, 253, 0.85)';
        ctx.lineWidth   = 1;
        ctx.fillRect(x0, 0, Math.max(1, x1 - x0), tl.height);
        ctx.beginPath();
        ctx.moveTo(x0, 0); ctx.lineTo(x0, tl.height);
        ctx.moveTo(x1, 0); ctx.lineTo(x1, tl.height);
        ctx.stroke();
        ctx.restore();
    }

    tl.addEventListener('mousedown', (e) => {
        if (!tl.__meta) return;
        dragging = true;
        const x  = pxFromEvent(e);
        const sel = selectionPx();
        if (sel && x >= sel.x0 && x <= sel.x1) {
            mode       = 'move';
            moveOffset = x - sel.x0;
            selWidth   = sel.x1 - sel.x0;
            curX       = x;
        } else {
            mode   = 'new';
            startX = x;
            curX   = x;
        }
        redrawWithOverlay();
    });

    window.addEventListener('mousemove', (e) => {
        if (!dragging) return;
        curX = pxFromEvent(e);
        redrawWithOverlay();
    });

    window.addEventListener('mouseup', async () => {
        if (!dragging) return;
        dragging = false;
        const m = tl.__meta;
        if (!m || data.xs.length < 2) {
            drawTimeline();
            return;
        }

        let x0;
        let x1;
        if (mode === 'move') {
            x0 = curX - moveOffset;
            x1 = x0 + selWidth;
        } else {
            x0 = Math.min(startX, curX);
            x1 = Math.max(startX, curX);
        }

        if (Math.abs(x1 - x0) < 6) {
            drawTimeline();
            return;
        }

        const t0 = tsFromCanvasX(tl, x0);
        const t1 = tsFromCanvasX(tl, x1);
        if (t0 === null || t1 === null) {
            drawTimeline();
            return;
        }

        const viewStart   = Math.min(t0, t1);
        const viewEnd     = Math.max(t0, t1);
        const selectedMs  = Math.max(1000, Math.floor(viewEnd - viewStart));
        windowMs          = selectedMs;
        followLive        = false;
        pausedEndTs       = Math.floor(viewEnd);

        const winSlider = document.getElementById('win-slider');
        const winLabel  = document.getElementById('win-slider-label');
        const minApprox = clamp(Math.round(selectedMs / 60000), 1, 60);
        winSlider.value  = String(minApprox);
        winLabel.textContent = `${minApprox}m*`;

        const startTs = data.xs[0];
        const spanSec = Math.max(0, Math.floor((data.xs[data.xs.length - 1] - startTs) / 1000));
        const endSec  = clamp(Math.floor((viewEnd - startTs) / 1000), 0, spanSec);
        const endSlider = document.getElementById('end-slider');
        endSlider.value  = String(endSec);
        updateEndSliderMax();

        const margin = 10_000;
        const qs = `?since_ms=${Math.max(0, Math.floor(viewStart - margin))}&until_ms=${Math.floor(viewEnd + margin)}&limit=50000`;
        const res = await fetch('/api/history' + qs);
        if (res.ok) {
            const hist = await res.json();
            resetData();
            if (Array.isArray(hist) && hist.length > 0) for (const p of hist) pushDataPoint(p);
            updateEndSliderMax();
        }
        redraw();
    });
}

// seriesSpec may be a static array or a zero-argument function that returns the array.
// Using a function allows specs that depend on runtime state (e.g. variable core count).
function installHoverTooltip(baseCanvas, overlayCanvas, seriesSpec) {
    function clear() {
        const ctx = overlayCanvas.getContext('2d');
        ctx.clearRect(0, 0, overlayCanvas.width, overlayCanvas.height);
        tooltip.style.display = 'none';
    }

    baseCanvas.addEventListener('mouseleave', clear);
    baseCanvas.addEventListener('mousemove', (e) => {
        if (!lastView || !baseCanvas.__meta || lastView.xs.length === 0) return;
        const rect = baseCanvas.getBoundingClientRect();
        const x    = e.clientX - rect.left;
        const ts   = tsFromCanvasX(baseCanvas, x);
        if (ts === null) return;
        const xs = lastView.xs;
        let i = lowerBound(xs, ts);
        if (i >= xs.length) i = xs.length - 1;
        if (i > 0) {
            const prev = xs[i - 1];
            const cur  = xs[i];
            if (Math.abs(ts - prev) < Math.abs(cur - ts)) i = i - 1;
        }

        const meta = baseCanvas.__meta;
        const xPx  = xToPxFromMeta(meta, xs[i]);

        // Draw overlay (crosshair + points).
        const ctx = overlayCanvas.getContext('2d');
        ctx.clearRect(0, 0, overlayCanvas.width, overlayCanvas.height);
        ctx.save();
        ctx.strokeStyle = 'rgba(156, 163, 175, 0.55)';
        ctx.lineWidth   = 1;
        ctx.beginPath();
        ctx.moveTo(xPx, 0);
        ctx.lineTo(xPx, overlayCanvas.height);
        ctx.stroke();

        const rows = [];
        rows.push(`<div style="color:#9ca3af;">${fmtTime(xs[i])}</div>`);

        // Resolve spec: support both a static array and a factory function.
        const resolvedSpec = typeof seriesSpec === 'function' ? seriesSpec() : seriesSpec;
        for (const spec of resolvedSpec) {
            const v   = spec.value(i);
            const yPx = yToPxFromMeta(meta, v);
            ctx.fillStyle = spec.color;
            ctx.beginPath();
            ctx.arc(xPx, yPx, 3, 0, Math.PI * 2);
            ctx.fill();
            rows.push(`<div><span style="color:${spec.color};">${spec.label}</span>: ${spec.fmt(v)}</div>`);
        }
        ctx.restore();

        // Tooltip.
        tooltip.innerHTML = rows.join('');
        tooltip.style.display = 'block';
        const pad = 12;
        const tw  = tooltip.offsetWidth;
        const th  = tooltip.offsetHeight;
        let left  = e.clientX + pad;
        let top   = e.clientY + pad;
        if (left + tw > window.innerWidth  - 8) left = e.clientX - tw - pad;
        if (top  + th > window.innerHeight - 8) top  = e.clientY - th - pad;
        tooltip.style.left = `${Math.max(8, left)}px`;
        tooltip.style.top  = `${Math.max(8, top)}px`;
    });
}

function installDragZoom(canvas) {
    let dragging = false;
    let startX   = 0;
    let curX     = 0;

    function drawOverlay() {
        if (!dragging) return;
        const ctx = canvas.getContext('2d');
        const x0  = Math.min(startX, curX);
        const x1  = Math.max(startX, curX);
        ctx.save();
        ctx.fillStyle   = 'rgba(59, 130, 246, 0.18)';
        ctx.strokeStyle = 'rgba(59, 130, 246, 0.7)';
        ctx.lineWidth   = 1;
        ctx.fillRect(x0, 0, Math.max(1, x1 - x0), canvas.height);
        ctx.beginPath();
        ctx.moveTo(x0, 0); ctx.lineTo(x0, canvas.height);
        ctx.moveTo(x1, 0); ctx.lineTo(x1, canvas.height);
        ctx.stroke();
        ctx.restore();
    }

    function redrawWithOverlay() {
        redraw();
        drawOverlay();
    }

    canvas.addEventListener('mousedown', (e) => {
        if (!canvas.__meta) return;
        dragging = true;
        const r  = canvas.getBoundingClientRect();
        startX   = e.clientX - r.left;
        curX     = startX;
        redrawWithOverlay();
    });

    window.addEventListener('mousemove', (e) => {
        if (!dragging) return;
        const r = canvas.getBoundingClientRect();
        curX    = e.clientX - r.left;
        redrawWithOverlay();
    });

    window.addEventListener('mouseup', async () => {
        if (!dragging) return;
        dragging = false;

        const x0 = Math.min(startX, curX);
        const x1 = Math.max(startX, curX);

        if (Math.abs(x1 - x0) < 4) {
            redraw();
            return;
        }

        const t0 = tsFromCanvasX(canvas, x0);
        const t1 = tsFromCanvasX(canvas, x1);
        if (t0 === null || t1 === null) {
            redraw();
            return;
        }

        const viewStart  = Math.min(t0, t1);
        const viewEnd    = Math.max(t0, t1);
        const selectedMs = Math.max(1000, Math.floor(viewEnd - viewStart));

        windowMs   = selectedMs;
        followLive = false;
        pausedEndTs = Math.floor(viewEnd);

        const winSlider = document.getElementById('win-slider');
        const winLabel  = document.getElementById('win-slider-label');
        const minApprox = clamp(Math.round(selectedMs / 60000), 1, 60);
        winSlider.value  = String(minApprox);
        winLabel.textContent = `${minApprox}m*`;

        if (data.xs.length >= 2) {
            const startTs = data.xs[0];
            const spanSec = Math.max(0, Math.floor((data.xs[data.xs.length - 1] - startTs) / 1000));
            const endSec  = clamp(Math.floor((viewEnd - startTs) / 1000), 0, spanSec);
            const endSlider = document.getElementById('end-slider');
            endSlider.value  = String(endSec);
            updateEndSliderMax();
        }

        await refetchForCurrentView();
        redraw();
    });

    canvas.addEventListener('dblclick', async () => {
        windowMs    = 180000;
        followLive  = true;
        pausedEndTs = null;
        const winSlider = document.getElementById('win-slider');
        const winLabel  = document.getElementById('win-slider-label');
        winSlider.value  = '3';
        winLabel.textContent = '3m';
        await refetchForCurrentView();
        redraw();
    });
}

async function refetchForCurrentView() {
    try {
        const marginMs = 10_000;
        let since = null;
        let until = null;
        if (windowMs !== 0) {
            const end = followLive ? Date.now() : (pausedEndTs ?? Date.now());
            since = Math.max(0, Math.floor(end - windowMs - marginMs));
            until = Math.max(0, Math.floor(end + marginMs));
        }
        const qs = since === null
            ? '?limit=10000'
            : `?since_ms=${since}&until_ms=${until}&limit=10000`;
        const resHist = await fetch('/api/history' + qs);
        if (!resHist.ok) throw new Error('HTTP ' + resHist.status);
        const hist = await resHist.json();
        resetData();
        if (Array.isArray(hist) && hist.length > 0) for (const p of hist) pushDataPoint(p);
        updateEndSliderMax();
        redraw();
    } catch (e) {
        document.getElementById('latest').textContent = 'Error: ' + e;
    }
}

function startStream() {
    const es = new EventSource('/api/stream');
    es.onmessage = (ev) => {
        try {
            const p = JSON.parse(ev.data);
            document.getElementById('latest').textContent = JSON.stringify(p, null, 2);
            pushDataPoint(p);
            updateStatCards(p);
            if (followLive) redraw();
        } catch (e) {

        }
    };
    es.onerror = () => {

    };
}

function initWindowControls() {
    const buttons = Array.from(document.querySelectorAll('button[data-win]'));
    function applyActive(ms) {
        for (const b of buttons) b.classList.toggle('active', Number(b.dataset.win) === ms);
        document.getElementById('range-label').textContent = windowLabel(ms);
    }
    for (const b of buttons) {
        b.addEventListener('click', async () => {
            const ms = Number(b.dataset.win);
            if (!Number.isFinite(ms)) return;
            windowMs = ms;
            applyActive(ms);
            const winSlider = document.getElementById('win-slider');
            const winLabel  = document.getElementById('win-slider-label');
            if (ms === 0) {
                winSlider.value  = '60';
                winLabel.textContent = 'All';
            } else {
                const min = Math.max(1, Math.round(ms / 60000));
                winSlider.value  = String(min);
                winLabel.textContent = `${min}m`;
            }
            await refetchForCurrentView();
        });
    }
    applyActive(windowMs);
}

function initSliders() {
    const winSlider = document.getElementById('win-slider');
    const winLabel  = document.getElementById('win-slider-label');
    winSlider.addEventListener('input', () => {
        const min = Number(winSlider.value);
        windowMs = min * 60000;
        winLabel.textContent = `${min}m`;
    });
    winSlider.addEventListener('change', async () => {
        const buttons = Array.from(document.querySelectorAll('button[data-win]'));
        for (const b of buttons) b.classList.remove('active');
        await refetchForCurrentView();
    });

    const endSlider = document.getElementById('end-slider');
    endSlider.addEventListener('input', () => {
        followLive = Number(endSlider.value || 0) === Number(endSlider.max || 0);
        if (!followLive && data.xs.length > 0) {
            const startTs = data.xs[0];
            pausedEndTs   = Math.floor(startTs + Number(endSlider.value || 0) * 1000);
        } else if (followLive) {
            pausedEndTs = null;
        }
        updateEndSliderMax();
        redraw();
    });

    const liveBtn = document.getElementById('live-btn');
    liveBtn.addEventListener('click', async () => {
        followLive  = true;
        pausedEndTs = null;
        updateEndSliderMax();
        await refetchForCurrentView();
        redraw();
    });
}

document.addEventListener('DOMContentLoaded', () => {
    initWindowControls();
    initSliders();
    refetchForCurrentView();
    startStream();

    installTimelineBrush();

    installHoverTooltip(document.getElementById('cpu'), document.getElementById('cpu-ov'), [
        { label: 'CPU', color: '#c44', value: (i) => lastView.cpu[i], fmt: (v) => `${v.toFixed(1)}%` },
    ]);
    installHoverTooltip(document.getElementById('mem'), document.getElementById('mem-ov'), [
        { label: 'Memory', color: '#f59e0b', value: (i) => lastView.mem[i], fmt: (v) => `${v.toFixed(1)}%` },
    ]);
    installHoverTooltip(document.getElementById('net'), document.getElementById('net-ov'), [
        { label: 'RX', color: '#0b6', value: (i) => lastView.rx[i], fmt: (v) => fmtBytes(v) },
        { label: 'TX', color: '#06b', value: (i) => lastView.tx[i], fmt: (v) => fmtBytes(v) },
    ]);
    installHoverTooltip(document.getElementById('load'), document.getElementById('load-ov'), [
        { label: '1m',  color: '#e879f9', value: (i) => lastView.la1[i],  fmt: (v) => v.toFixed(2) },
        { label: '5m',  color: '#a78bfa', value: (i) => lastView.la5[i],  fmt: (v) => v.toFixed(2) },
        { label: '15m', color: '#60a5fa', value: (i) => lastView.la15[i], fmt: (v) => v.toFixed(2) },
    ]);

    installHoverTooltip(document.getElementById('cores'), document.getElementById('cores-ov'), () => {
        const n = lastView ? lastView.coreLines.length : 0;
        const specs = [];
        for (let c = 0; c < n; c++) {
            const cc = c;
            specs.push({
                label: `C${cc}`,
                color: coreColor(cc, n),
                value: (i) => (lastView.coreLines[cc] || [])[i] ?? 0,
                fmt:   (v) => `${v.toFixed(1)}%`,
            });
        }
        return specs;
    });
    installHoverTooltip(document.getElementById('disk'), document.getElementById('disk-ov'), [
        { label: 'Disk used', color: '#34d399', value: (i) => lastView.disk[i], fmt: (v) => `${v.toFixed(1)}%` },
    ]);
    installHoverTooltip(document.getElementById('swap'), document.getElementById('swap-ov'), [
        { label: 'Swap', color: '#818cf8', value: (i) => lastView.swap[i], fmt: (v) => `${v.toFixed(1)}%` },
    ]);


    for (const id of ['cpu', 'mem', 'net', 'load', 'cores', 'disk', 'swap']) {
        installDragZoom(document.getElementById(id));
    }
});