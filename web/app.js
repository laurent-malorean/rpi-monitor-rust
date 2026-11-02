'use strict';
// Interface Web de rpi-monitor : sans dépendance, sans HTML dynamique (les valeurs viennent du serveur).

const PERIODS = [
  { s: 900, label: '15 min' },
  { s: 3600, label: '1 h' },
  { s: 21600, label: '6 h' },
  { s: 86400, label: '24 h' },
  { s: 172800, label: '48 h' },
];
const DEFAULT_PERIOD = 3600;
const LOST_AFTER_MS = 5000;

// kind : courbe (« line ») ou barres d'activité 0..1 (« bar ») ; range : plage fixe, sinon automatique.
const METRICS = [
  { id: 'cpu', kind: 'line', range: [0, 100], decimals: 0 },
  { id: 'freq', kind: 'line', range: null, decimals: 0 },
  { id: 'power', kind: 'line', range: null, decimals: 0 },
  { id: 'ram', kind: 'line', range: [0, 100], decimals: 0 },
  { id: 'temp', kind: 'line', range: null, decimals: 1 },
  { id: 'fan', kind: 'bar', range: [0, 1], decimals: 0 },
  { id: 'throttle', kind: 'bar', range: [0, 1], decimals: 0 },
];

// ---------- logique pure (testée avec node) ----------

/** Valeur affichée d'un indicateur : entier arrondi au supérieur, sauf la température (1 décimale). */
function fmtValue(v, m) {
  if (v === null || v === undefined || Number.isNaN(v)) return '--';
  return m.decimals === 0 ? String(Math.ceil(v)) : Number(v).toFixed(m.decimals);
}

function fmt(v, decimals) {
  return v === null || v === undefined || Number.isNaN(v) ? '--' : Number(v).toFixed(decimals);
}

/** Coupe la série là où il manque des mesures : un trou n'est jamais relié ni rempli de zéros. */
function segments(points, step) {
  const out = [];
  let cur = [];
  for (const p of points) {
    if (cur.length && p[0] - cur[cur.length - 1][0] > step * 2.5) {
      out.push(cur);
      cur = [];
    }
    cur.push(p);
  }
  if (cur.length) out.push(cur);
  return out;
}

function extent(points, fixed) {
  if (fixed) return fixed;
  if (!points.length) return [0, 1];
  let lo = Infinity;
  let hi = -Infinity;
  for (const p of points) {
    lo = Math.min(lo, p[2]);
    hi = Math.max(hi, p[3]);
  }
  const pad = hi > lo ? (hi - lo) * 0.1 : Math.max(Math.abs(hi) * 0.1, 1);
  return [lo - pad, hi + pad];
}

function isConnectionLost(lastOkMs, nowMs) {
  return !lastOkMs || nowMs - lastOkMs > LOST_AFTER_MS;
}

function formatDuration(s) {
  if (s < 90) return Math.round(s) + ' s';
  const min = Math.round(s / 60);
  if (min < 90) return min + ' min';
  const h = Math.floor(min / 60);
  const m = min % 60;
  return m ? h + ' h ' + String(m).padStart(2, '0') : h + ' h';
}

function pathFor(seg, x, y) {
  return seg.map((p, i) => (i ? 'L' : 'M') + x(p[0]).toFixed(1) + ' ' + y(p[1]).toFixed(1)).join('');
}

function eventLines(events, limit) {
  return events
    .slice()
    .reverse()
    .slice(0, limit)
    .map((e) => ({ start: e.start, end: e.end, duration: e.end - e.start, peak: e.peak }));
}

// ---------- affichage ----------

const state = {
  period: DEFAULT_PERIOD,
  current: null,
  history: null,
  stats: null,
  events: null,
  lastOk: 0,
  needLogin: false,
  inflight: { current: false, slow: false },
  lastSlow: 0,
  meta: {},
};

function h(tag, props, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (k === 'class') el.className = v;
    else if (v !== undefined && v !== null) el.setAttribute(k, v);
  }
  for (const c of children.flat()) {
    if (c === null || c === undefined || c === false) continue;
    el.append(c.nodeType ? c : document.createTextNode(String(c)));
  }
  return el;
}

const SVG = ['http:', '', 'www.w3.org', '2000', 'svg'].join('/'); // espace de noms XML, pas une ressource chargée
function s(tag, props) {
  const el = document.createElementNS(SVG, tag);
  for (const [k, v] of Object.entries(props || {})) el.setAttribute(k, v);
  return el;
}

function timeLabel(ts, period) {
  const d = new Date(ts * 1000);
  const hm = d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  return period > 86400 ? d.toLocaleDateString([], { day: '2-digit', month: '2-digit' }) + ' ' + hm : hm;
}

const W = 600;
const H = 120;

function drawChart(box, m, points, step, period, now) {
  box.textContent = '';
  const t0 = now - period;
  const x = (ts) => ((ts - t0) / period) * W;
  const [lo, hi] = extent(points, m.range);
  const y = (v) => H - 3 - ((v - lo) / (hi - lo || 1)) * (H - 6);
  const svg = s('svg', { viewBox: `0 0 ${W} ${H}`, preserveAspectRatio: 'none', role: 'img' });
  for (const f of [0.25, 0.5, 0.75]) svg.append(s('line', { class: 'grid', x1: 0, x2: W, y1: H * f, y2: H * f }));
  const color = `var(--c-${m.id})`;
  if (m.kind === 'bar') {
    for (const p of points) {
      const bh = Math.max(p[1] > 0 ? 3 : 0, p[1] * (H - 6));
      svg.append(s('rect', { class: 'bar', x: x(p[0]).toFixed(1), y: (H - 3 - bh).toFixed(1), width: Math.max(1.5, (step / period) * W).toFixed(1), height: bh.toFixed(1), fill: color }));
    }
  } else {
    for (const seg of segments(points, step)) {
      const top = seg.map((p, i) => (i ? 'L' : 'M') + x(p[0]).toFixed(1) + ' ' + y(p[3]).toFixed(1)).join('');
      const bottom = seg.slice().reverse().map((p) => 'L' + x(p[0]).toFixed(1) + ' ' + y(p[2]).toFixed(1)).join('');
      svg.append(s('path', { class: 'band', d: top + bottom + 'Z', fill: color }));
      svg.append(s('path', { class: 'line', d: pathFor(seg, x, y), stroke: color }));
      if (seg.length === 1) svg.append(s('circle', { cx: x(seg[0][0]), cy: y(seg[0][1]), r: 2, fill: color }));
    }
  }
  const cursor = s('line', { class: 'cursor', y1: 0, y2: H, x1: 0, x2: 0 });
  cursor.setAttribute('visibility', 'hidden');
  svg.append(cursor);
  box.append(svg);
  if (m.kind === 'line') {
    box.append(h('span', { class: 'ylab top' }, fmt(hi, m.decimals)), h('span', { class: 'ylab bottom' }, fmt(lo, m.decimals)));
  }
  if (!points.length) box.append(h('div', { class: 'nodata' }, 'Aucune donnée sur la période'));

  const tip = h('div', { class: 'tip' });
  tip.hidden = true;
  box.append(tip);
  const move = (ev) => {
    const r = svg.getBoundingClientRect();
    const ts = t0 + ((ev.clientX - r.left) / r.width) * period;
    let best = null;
    for (const p of points) if (!best || Math.abs(p[0] - ts) < Math.abs(best[0] - ts)) best = p;
    if (!best || Math.abs(best[0] - ts) > step * 2.5) return leave();
    cursor.setAttribute('x1', x(best[0]));
    cursor.setAttribute('x2', x(best[0]));
    cursor.setAttribute('visibility', 'visible');
    const val = m.kind === 'bar' ? Math.round(best[1] * 100) + ' % du temps' : fmt(best[1], m.decimals) + ' ' + (state.meta[m.id] ? state.meta[m.id].unit : '');
    tip.textContent = timeLabel(best[0], period) + ' · ' + val;
    tip.hidden = false;
    const left = ((x(best[0]) / W) * r.width);
    tip.style.left = Math.min(Math.max(left - 40, 0), r.width - tip.offsetWidth) + 'px';
  };
  const leave = () => {
    cursor.setAttribute('visibility', 'hidden');
    tip.hidden = true;
  };
  svg.addEventListener('pointermove', move);
  svg.addEventListener('pointerleave', leave);
}

function eventList(events, period) {
  const lines = eventLines(events || [], 5);
  if (!lines.length) return h('ul', { class: 'events' }, h('li', { class: 'none' }, 'Aucun événement sur la période'));
  return h('ul', { class: 'events' }, lines.map((e) => h('li', null, timeLabel(e.start, period) + ' → ' + timeLabel(e.end, period) + ' (' + formatDuration(e.duration) + ')')));
}

const cards = {};

function ensureCards() {
  const main = document.getElementById('app');
  if (main.childElementCount) return;
  for (const m of METRICS) {
    const c = {
      title: h('span'),
      info: h('span'),
      value: h('div', { class: 'value na' }, '--'),
      stats: h('div', { class: 'stats' }),
      detail: h('div'),
      chart: h('div', { class: 'chart' }),
      axis: h('div', { class: 'axis' }, h('span'), h('span')),
      events: h('div'),
    };
    const el = h('section', { class: 'card', id: 'card-' + m.id }, h('h2', null, c.title, c.info), h('div', { class: 'head' }, c.value, c.stats), c.detail, c.chart, c.axis, c.events);
    cards[m.id] = c;
    main.append(el);
  }
}

function setValue(c, text, unit, available) {
  c.value.className = available ? 'value' : 'value na';
  c.value.textContent = '';
  c.value.append(available ? text : 'indisponible');
  if (available && unit) c.value.append(h('small', null, unit));
}

function render() {
  ensureCards();
  const cur = state.current;
  const now = cur ? cur.ts : Date.now() / 1000;
  document.getElementById('demo').hidden = !(cur && cur.demo);
  for (const m of METRICS) {
    const c = cards[m.id];
    const info = cur && cur.metrics[m.id];
    if (info) state.meta[m.id] = info;
    c.title.textContent = info ? info.label : m.id;
    if (info) {
      if (m.id === 'fan') {
        setValue(c, info.active ? 'Actif' : 'Arrêté', '', info.active !== null);
        c.detail.textContent = '';
        c.detail.append(h('div', { class: 'detail' }, `niveau ${fmt(info.state, 0)} · PWM ${fmt(info.pwm, 0)} · ${fmt(info.rpm, 0)} tr/min`));
      } else if (m.id === 'throttle') {
        setValue(c, info.active ? 'En cours' : 'Aucun', '', info.active !== null);
        c.detail.textContent = '';
        const pills = [...(info.now || []).map((f) => h('span', { class: 'pill bad' }, f)), ...(info.since_boot || []).filter((f) => !(info.now || []).includes(f)).map((f) => h('span', { class: 'pill' }, f + ' (depuis le boot)'))];
        c.detail.append(h('div', null, pills));
      } else {
        setValue(c, fmtValue(info.value, m), info.unit, info.value !== null);
      }
    }
    const st = state.stats && state.stats.stats[m.id];
    c.stats.textContent = '';
    if (m.kind === 'line') {
      c.stats.append(...(st ? [h('span', null, 'min ', h('b', null, fmtValue(st.min, m))), h('span', null, 'moy ', h('b', null, fmtValue(st.mean, m))), h('span', null, 'max ', h('b', null, fmtValue(st.max, m)))] : [h('span', null, 'indisponible')]));
    } else if (st) {
      c.stats.append(h('span', null, 'actif ', h('b', null, Math.round(st.mean * 100) + ' %')), h('span', null, 'du temps'));
    }
    if (state.history && state.history.series[m.id]) {
      drawChart(c.chart, m, state.history.series[m.id], state.history.step_s, state.history.period_s, now);
      const [a, b] = c.axis.children;
      a.textContent = timeLabel(now - state.history.period_s, state.history.period_s);
      b.textContent = 'maintenant';
    }
    if (m.kind === 'bar' && state.events) {
      c.events.textContent = '';
      c.events.append(eventList(state.events.events[m.id], state.history ? state.history.period_s : state.period));
    }
  }
}

// ---------- réseau ----------

class AuthError extends Error {}

async function api(path) {
  const ctl = new AbortController();
  const timer = setTimeout(() => ctl.abort(), 4000);
  try {
    const r = await fetch(path, { credentials: 'same-origin', cache: 'no-store', signal: ctl.signal });
    if (r.status === 401) throw new AuthError();
    if (!r.ok) throw new Error('HTTP ' + r.status);
    return await r.json();
  } finally {
    clearTimeout(timer);
  }
}

function showLogin(on) {
  state.needLogin = on;
  document.getElementById('login').hidden = !on;
  document.getElementById('app').hidden = on;
  document.getElementById('logout').hidden = on;
  if (on) document.getElementById('password').focus();
}

function updateStatus() {
  const lost = isConnectionLost(state.lastOk, Date.now());
  const st = document.getElementById('status');
  st.className = 'status ' + (state.needLogin ? '' : lost ? 'lost' : 'ok');
  document.getElementById('status-text').textContent = state.needLogin ? 'Connexion requise' : lost ? 'Connexion perdue' : 'Connecté';
  const banner = document.getElementById('banner');
  banner.hidden = state.needLogin || !lost;
  banner.textContent = state.lastOk ? 'Connexion avec le serveur perdue : nouvelle tentative chaque seconde. Les valeurs affichées ne sont plus à jour.' : 'Impossible de joindre le serveur.';
  document.getElementById('app').classList.toggle('stale', lost && !state.needLogin);
}

async function guarded(kind, fn) {
  if (state.inflight[kind] || document.hidden || state.needLogin) return;
  state.inflight[kind] = true;
  try {
    await fn();
    state.lastOk = Date.now();
  } catch (e) {
    if (e instanceof AuthError) showLogin(true);
  } finally {
    state.inflight[kind] = false;
  }
}

const refreshCurrent = () => guarded('current', async () => {
  state.current = await api('/api/v1/metrics/current');
  render();
});

const refreshSlow = () => guarded('slow', async () => {
  const p = state.period;
  const [history, stats, events] = await Promise.all([
    api('/api/v1/metrics/history?period=' + p),
    api('/api/v1/metrics/stats?period=' + p),
    api('/api/v1/metrics/events?period=' + p),
  ]);
  if (p !== state.period) return; // la période a changé pendant la requête
  Object.assign(state, { history, stats, events, lastSlow: Date.now() });
  render();
});

function tick() {
  refreshCurrent();
  const every = state.period <= 3600 ? 10000 : 30000;
  if (Date.now() - state.lastSlow >= every) refreshSlow();
  updateStatus();
}

function setPeriod(sec) {
  state.period = sec;
  state.lastSlow = 0;
  try { localStorage.setItem('rpimon.period', String(sec)); } catch (_) { /* stockage indisponible : sans effet */ }
  for (const b of document.querySelectorAll('#periods button')) b.setAttribute('aria-pressed', String(Number(b.dataset.s) === sec));
  refreshSlow();
}

function start() {
  try {
    const saved = Number(localStorage.getItem('rpimon.period'));
    if (PERIODS.some((p) => p.s === saved)) state.period = saved;
  } catch (_) { /* ignoré */ }
  const nav = document.getElementById('periods');
  for (const p of PERIODS) nav.append(h('button', { type: 'button', 'data-s': p.s, 'aria-pressed': String(p.s === state.period) }, p.label));
  nav.addEventListener('click', (e) => {
    const b = e.target.closest('button');
    if (b) setPeriod(Number(b.dataset.s));
  });
  document.getElementById('login-form').addEventListener('submit', async (e) => {
    e.preventDefault();
    const err = document.getElementById('login-error');
    err.textContent = '';
    try {
      const r = await fetch('/api/v1/login', { method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ password: document.getElementById('password').value }) });
      if (!r.ok) {
        const body = await r.json().catch(() => ({}));
        err.textContent = body.error || 'Échec de la connexion';
        return;
      }
      document.getElementById('password').value = '';
      showLogin(false);
      state.lastSlow = 0;
      tick();
    } catch (_) {
      err.textContent = 'Serveur injoignable';
    }
  });
  document.getElementById('logout').addEventListener('click', async () => {
    try { await fetch('/api/v1/logout', { method: 'POST', credentials: 'same-origin' }); } catch (_) { /* ignoré */ }
    showLogin(true);
  });
  document.addEventListener('visibilitychange', () => { if (!document.hidden) { state.lastSlow = 0; tick(); } });
  document.getElementById('logout').hidden = false;
  tick();
  setInterval(tick, 1000);
}

if (typeof module !== 'undefined' && module.exports) {
  module.exports = { fmtValue, fmt, segments, extent, isConnectionLost, formatDuration, pathFor, eventLines, PERIODS, DEFAULT_PERIOD, METRICS };
} else {
  start();
}
