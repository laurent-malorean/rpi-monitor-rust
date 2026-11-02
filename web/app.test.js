// Tests de la logique pure de l'interface (node web/app.test.js). Lancés par `cargo test` si node est présent.
const assert = require('node:assert/strict');
const t = require('./app.js');

const tests = {
  'fmt : null et NaN donnent -- (jamais 0)'() {
    assert.equal(t.fmt(null, 1), '--');
    assert.equal(t.fmt(undefined, 1), '--');
    assert.equal(t.fmt(NaN, 1), '--');
    assert.equal(t.fmt(0, 1), '0.0');
    assert.equal(t.fmt(41.256, 1), '41.3');
  },
  'fmtValue : entier arrondi au supérieur, sauf la température (1 décimale)'() {
    const by = (id) => t.METRICS.find((m) => m.id === id);
    assert.equal(t.fmtValue(0.3, by('cpu')), '1');
    assert.equal(t.fmtValue(14.7, by('cpu')), '15');
    assert.equal(t.fmtValue(12, by('cpu')), '12', 'une valeur entière ne monte pas');
    assert.equal(t.fmtValue(0, by('cpu')), '0');
    assert.equal(t.fmtValue(1513.6, by('freq')), '1514');
    assert.equal(t.fmtValue(46.34, by('temp')), '46.3');
    assert.equal(t.fmtValue(null, by('cpu')), '--', 'indisponible, jamais 0');
    assert.equal(t.fmtValue(NaN, by('temp')), '--');
  },
  'coreColor / coreLabel : une couleur par cœur, qui reboucle au-delà de 8'() {
    assert.equal(t.coreColor(0), 'var(--c-core0)');
    assert.equal(t.coreColor(3), 'var(--c-core3)');
    assert.equal(t.coreColor(8), 'var(--c-core0)');
    assert.equal(t.coreLabel(0), 'Cœur 1');
    assert.equal(t.coreLabel(3), 'Cœur 4');
  },
  'nearestPoint : le point le plus proche, ou rien dans un trou'() {
    const pts = [[0, 1, 1, 1], [10, 2, 2, 2], [5000, 3, 3, 3]];
    assert.equal(t.nearestPoint(pts, 11, 10)[1], 2);
    assert.equal(t.nearestPoint(pts, 2500, 10), null, 'au milieu d un trou');
    assert.equal(t.nearestPoint([], 5, 10), null);
  },
  'segments : un trou coupe la courbe au lieu de la relier'() {
    const pts = [[0, 1, 1, 1], [10, 2, 2, 2], [20, 3, 3, 3], [5000, 4, 4, 4], [5010, 5, 5, 5]];
    const segs = t.segments(pts, 10);
    assert.equal(segs.length, 2);
    assert.deepEqual(segs.map((s) => s.length), [3, 2]);
    assert.deepEqual(t.segments([], 10), []);
  },
  'segments : un pas rééchantillonné tolère un trou de deux pas'() {
    const pts = [[0, 1, 1, 1], [350, 2, 2, 2], [700, 3, 3, 3]];
    assert.equal(t.segments(pts, 350).length, 1);
  },
  'extent : plage fixe pour les pourcentages, auto sinon, jamais vide'() {
    assert.deepEqual(t.extent([[0, 5, 1, 9]], [0, 100]), [0, 100]);
    const [lo, hi] = t.extent([[0, 40, 38, 45], [10, 41, 39, 50]], null);
    assert.ok(lo <= 38 && hi >= 50 && hi > lo);
    const [a, b] = t.extent([[0, 40, 40, 40]], null);
    assert.ok(b > a, 'plage non nulle pour une valeur constante');
    assert.deepEqual(t.extent([], null), [0, 1]);
  },
  'connexion perdue après 5 s sans réponse'() {
    assert.equal(t.isConnectionLost(1000, 5000), false);
    assert.equal(t.isConnectionLost(1000, 7000), true);
    assert.equal(t.isConnectionLost(0, 7000), true, 'jamais connecté');
  },
  'formatDuration'() {
    assert.equal(t.formatDuration(30), '30 s');
    assert.equal(t.formatDuration(600), '10 min');
    assert.equal(t.formatDuration(5400), '1 h 30');
    assert.equal(t.formatDuration(7200), '2 h');
  },
  'pathFor : M puis L, coordonnées bornées'() {
    const d = t.pathFor([[0, 10, 0, 0], [10, 20, 0, 0]], (ts) => ts * 2, (v) => 100 - v);
    assert.equal(d, 'M0.0 90.0L20.0 80.0');
  },
  'periods : 15 min à 48 h, 1 h par défaut'() {
    assert.deepEqual(t.PERIODS.map((p) => p.s), [900, 3600, 21600, 86400, 172800]);
    assert.equal(t.DEFAULT_PERIOD, 3600);
  },
  'metrics : les sept indicateurs dans l ordre de l API'() {
    assert.deepEqual(t.METRICS.map((m) => m.id), ['cpu', 'freq', 'power', 'ram', 'temp', 'fan', 'throttle']);
    assert.deepEqual(t.METRICS.filter((m) => m.decimals > 0).map((m) => m.id), ['temp'], 'seule la température garde une décimale');
  },
  'eventLines : plus récents d abord, limités'() {
    const ev = Array.from({ length: 8 }, (_, i) => ({ start: i * 100, end: i * 100 + 60, peak: 1 }));
    const lines = t.eventLines(ev, 5);
    assert.equal(lines.length, 5);
    assert.equal(lines[0].start, 700);
    assert.equal(lines[0].duration, 60);
  },
};

let failed = 0;
for (const [name, fn] of Object.entries(tests)) {
  try { fn(); console.log('ok  -', name); } catch (e) { failed++; console.log('FAIL-', name, '\n', e.message); }
}
process.exit(failed ? 1 : 0);
