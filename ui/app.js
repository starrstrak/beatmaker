'use strict';
// BeatMaker UI (phase 1). Plain JS on purpose: one screen, no build step.
// All sound runs in the Rust engine; this file only sends commands and draws state.

const invoke = window.__TAURI__ && window.__TAURI__.core ? window.__TAURI__.core.invoke : null;
const $ = (id) => document.getElementById(id);
const PPQ = 960;

// ---------- UI text (French). Engine parameter keys map to label + beginner tooltip. ----------
const WAVES = ['Sinus', 'Triangle', 'Dent de scie', 'Carré'];
const GROUPS = { osc: 'Oscillateurs', filter: 'Filtre', amp: 'Volume (enveloppe)', fenv: 'Enveloppe du filtre', perf: 'Jeu', master: 'Sortie' };
const TEXT = {
  osc1_wave: ['Forme d\'onde', 'Le timbre de base : sinus = doux, triangle = rond, dent de scie = riche et brillant, carré = creux.'],
  osc2_wave: ['Forme d\'onde 2', 'Forme du deuxième oscillateur, mélangé au premier.'],
  osc2_detune: ['Désaccord', 'Désaccorde légèrement le 2e oscillateur : le son devient plus épais (effet « chorus »).'],
  osc2_level: ['Niveau osc. 2', 'Quantité du deuxième oscillateur dans le mélange.'],
  cutoff: ['Coupure', 'Filtre passe-bas : plus c\'est bas, plus le son est sourd ; plus c\'est haut, plus il est brillant.'],
  resonance: ['Résonance', 'Accentue les fréquences autour de la coupure : son plus « vocal » ou sifflant.'],
  filter_env: ['Intensité env. filtre', 'De combien l\'enveloppe ouvre ou ferme le filtre à chaque note (en octaves).'],
  amp_attack: ['Attaque', 'Temps pour que le son atteigne son volume maximal. Court = percussif, long = progressif.'],
  amp_decay: ['Déclin', 'Temps pour redescendre vers le niveau de maintien.'],
  amp_sustain: ['Maintien', 'Niveau tenu tant que la touche reste enfoncée.'],
  amp_release: ['Relâchement', 'Durée pendant laquelle le son s\'éteint après avoir lâché la touche.'],
  flt_attack: ['Attaque filtre', 'Vitesse d\'ouverture du filtre.'],
  flt_decay: ['Déclin filtre', 'Vitesse de retombée du filtre.'],
  flt_sustain: ['Maintien filtre', 'Niveau du filtre tant que la touche est tenue.'],
  flt_release: ['Relâchement filtre', 'Vitesse de fermeture du filtre après la touche.'],
  bend_range: ['Plage pitch bend', 'Étendue de la molette de hauteur, en demi-tons.'],
  mod_depth: ['Vibrato (molette)', 'Profondeur du vibrato commandé par la molette de modulation et l\'aftertouch.'],
  master: ['Volume général', 'Volume de sortie du synthé.'],
  metro_volume: ['Volume métronome', 'Volume du clic de métronome.'],
};

// ---------- helpers ----------
const store = {
  get(k, d) { try { return localStorage.getItem(k) ?? d; } catch { return d; } },
  set(k, v) { try { localStorage.setItem(k, v); } catch { /* storage unavailable: ignore */ } },
};
async function call(cmd, args) {
  if (!invoke) throw new Error('Lance l\'application avec « cargo tauri dev » (pas dans un navigateur).');
  return invoke(cmd, args || {});
}
function setStatus(el, text, err) { el.textContent = text; el.classList.toggle('err', !!err); }
const dbToPct = (v) => (v <= 0.0001 ? 0 : Math.max(0, Math.min(1, (20 * Math.log10(v) + 60) / 60)));

// ---------- mode & theme ----------
function applyMode(m) {
  document.documentElement.dataset.mode = m;
  $('mode-beginner').classList.toggle('active', m === 'beginner');
  $('mode-pro').classList.toggle('active', m === 'pro');
  store.set('mode', m);
}
$('mode-beginner').onclick = () => applyMode('beginner');
$('mode-pro').onclick = () => applyMode('pro');
applyMode(store.get('mode', 'beginner'));
document.documentElement.dataset.theme = store.get('theme', 'dark');
$('theme').onclick = () => {
  const t = document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark';
  document.documentElement.dataset.theme = t;
  store.set('theme', t);
};

// ---------- audio setup ----------
let devices = [];
async function loadDevices() {
  devices = await call('list_audio_devices');
  const sel = $('dev');
  sel.innerHTML = '';
  devices.forEach((d, i) => {
    const o = new Option(d.name + (d.is_default ? ' (par défaut)' : ''), i);
    sel.add(o);
    if (d.is_default) sel.value = i;
  });
  fillRates();
}
function fillRates() {
  const d = devices[Number($('dev').value)];
  const rate = $('rate');
  rate.innerHTML = '';
  if (!d) return;
  d.sample_rates.forEach((r) => rate.add(new Option(r.toLocaleString('fr-FR') + ' Hz', r)));
  rate.value = d.sample_rates.includes(d.default_sample_rate) ? d.default_sample_rate : d.sample_rates[0];
}
$('dev').onchange = fillRates;
async function applyAudio() {
  const d = devices[Number($('dev').value)];
  try {
    const buf = $('buf').value;
    const info = await call('start_audio', {
      device: d ? d.name : null,
      sampleRate: Number($('rate').value) || null,
      bufferFrames: buf ? Number(buf) : null,
    });
    const lat = info.latency_ms != null ? ` · latence ≈ ${info.latency_ms.toFixed(1)} ms` : '';
    setStatus($('audio-status'), `Audio actif : ${info.device}, ${info.sample_rate} Hz, ${info.channels} canaux${lat}`);
  } catch (e) {
    setStatus($('audio-status'), String(e), true);
  }
}
$('apply-audio').onclick = applyAudio;

// ---------- transport ----------
async function transport(action) {
  try { await call('transport', { action }); } catch (e) { setStatus($('audio-status'), String(e), true); }
}
$('btn-rec').onclick = () => transport('record');
$('btn-play').onclick = () => transport('play');
$('btn-stop').onclick = () => transport('stop');
$('tempo').onchange = (e) => call('set_tempo', { bpm: Number(e.target.value) });
const sendSig = () => call('set_time_signature', { numerator: Number($('num').value), denominator: Number($('den').value) });
$('num').onchange = sendSig;
$('den').onchange = sendSig;
$('countin').onchange = (e) => call('set_count_in', { beats: Number(e.target.value) });
$('metro').onchange = (e) => call('set_metronome', { on: e.target.checked });

let last = { playing: false, recording: false };
$('btn-export').onclick = async () => {
  const out = $('export-status');
  setStatus(out, 'Rendu en cours…');
  try {
    const r = await call('export_wav', { sampleRate: Number($('xrate').value), bits: Number($('xbits').value), dither: $('xdither').checked });
    setStatus(out, `Exporté (${r.seconds.toFixed(1)} s, crête ${(20 * Math.log10(Math.max(r.peak, 1e-6))).toFixed(1)} dBFS) :\n${r.path}`);
  } catch (e) { setStatus(out, String(e), true); }
};

// ---------- synth controls ----------
let params = [];
const toNorm = (p, v) => {
  if (p.curve === 'Log') return Math.log(v / p.min) / Math.log(p.max / p.min);
  return (v - p.min) / (p.max - p.min);
};
const fromNorm = (p, n) => {
  if (p.curve === 'Log') return p.min * Math.pow(p.max / p.min, n);
  const v = p.min + n * (p.max - p.min);
  return p.curve === 'Choice' ? Math.round(v) : v;
};
function fmt(p, v) {
  if (p.curve === 'Choice') return WAVES[Math.round(v)];
  if (p.unit === 'Hz') return v >= 1000 ? (v / 1000).toFixed(1) + ' kHz' : Math.round(v) + ' Hz';
  if (p.unit === 's') return v < 1 ? Math.round(v * 1000) + ' ms' : v.toFixed(2) + ' s';
  if (p.unit === 'ct') return Math.round(v) + ' ct';
  if (p.unit === 'oct') return v.toFixed(1) + ' oct';
  if (p.unit === 'st') return v.toFixed(1) + ' st';
  return Math.round(v * 100) + ' %';
}
async function buildSynth() {
  params = await call('get_params');
  const root = $('synth-controls');
  root.innerHTML = '';
  for (const g of Object.keys(GROUPS)) {
    const list = params.filter((p) => p.group === g);
    if (!list.length) continue;
    const box = document.createElement('div');
    box.className = 'group' + (list.some((p) => p.basic) ? '' : ' pro');
    box.innerHTML = `<h4>${GROUPS[g]}</h4>`;
    for (const p of list) {
      const [label, tip] = TEXT[p.key] || [p.key, ''];
      const row = document.createElement('div');
      row.className = 'ctl' + (p.basic ? '' : ' pro');
      row.title = tip;
      const out = document.createElement('output');
      out.textContent = fmt(p, p.value);
      const lab = document.createElement('label');
      lab.textContent = label;
      row.append(lab, out);
      if (p.curve === 'Choice') {
        const s = document.createElement('select');
        WAVES.forEach((w, i) => s.add(new Option(w, i)));
        s.value = Math.round(p.value);
        s.onchange = () => { out.textContent = fmt(p, Number(s.value)); call('set_param', { key: p.key, value: Number(s.value) }); };
        row.append(s);
      } else {
        const r = document.createElement('input');
        r.type = 'range'; r.min = 0; r.max = 1000; r.value = Math.round(toNorm(p, p.value) * 1000);
        r.oninput = () => {
          const v = fromNorm(p, r.value / 1000);
          out.textContent = fmt(p, v);
          call('set_param', { key: p.key, value: v });
        };
        row.append(r);
      }
      box.append(row);
    }
    root.append(box);
  }
}

// ---------- on-screen + computer keyboard ----------
let octave = 3;
let velocity = 100;
const KEYMAP = { KeyA: 0, KeyW: 1, KeyS: 2, KeyE: 3, KeyD: 4, KeyF: 5, KeyT: 6, KeyG: 7, KeyY: 8, KeyH: 9, KeyU: 10, KeyJ: 11, KeyK: 12, KeyO: 13, KeyL: 14, KeyP: 15, Semicolon: 16 };
const NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];
const isBlack = (n) => [1, 3, 6, 8, 10].includes(n % 12);
const noteName = (n) => NAMES[n % 12] + (Math.floor(n / 12) - 1);
const held = new Map(); // key id -> midi note
const keyEls = new Map();

function noteOn(n) { call('note', { note: n, velocity, on: true }).catch(() => {}); const el = keyEls.get(n); if (el) el.classList.add('down'); }
function noteOff(n) { call('note', { note: n, velocity: 0, on: false }).catch(() => {}); const el = keyEls.get(n); if (el) el.classList.remove('down'); }

function buildPiano() {
  const root = $('piano');
  root.innerHTML = '';
  const first = 36, last = 84; // C2..C6
  const whites = [];
  for (let n = first; n <= last; n++) if (!isBlack(n)) whites.push(n);
  const w = 100 / whites.length;
  let wi = 0;
  for (let n = first; n <= last; n++) {
    const el = document.createElement('div');
    if (isBlack(n)) {
      el.className = 'pk b';
      el.style.left = (wi * w - w * 0.3) + '%';
      el.style.width = (w * 0.6) + '%';
    } else {
      el.className = 'pk w';
      el.style.left = (wi * w) + '%';
      el.style.width = w + '%';
      if (n % 12 === 0) el.innerHTML = `<span>${noteName(n)}</span>`;
      wi++;
    }
    el.onpointerdown = (e) => { e.preventDefault(); el.setPointerCapture(e.pointerId); held.set('p' + n, n); noteOn(n); };
    el.onpointerup = el.onpointercancel = () => { if (held.delete('p' + n)) noteOff(n); };
    keyEls.set(n, el);
    root.append(el);
  }
}
function updateKbdInfo() { $('oct').textContent = noteName(12 * (octave + 1)); $('vel').textContent = velocity; }

const typing = (e) => /^(INPUT|SELECT|TEXTAREA)$/.test(e.target.tagName) && e.target.type !== 'range';
window.addEventListener('keydown', (e) => {
  if (e.metaKey || e.ctrlKey) return;
  if (e.code === 'AltLeft' || e.code === 'AltRight') { e.preventDefault(); if (!e.repeat) call('sustain', { down: true }); return; }
  if (typing(e)) { if (e.code === 'Escape') e.target.blur(); return; }
  if (e.repeat) return;
  if (e.code in KEYMAP) {
    const n = 12 * (octave + 1) + KEYMAP[e.code];
    if (n <= 127 && !held.has(e.code)) { held.set(e.code, n); noteOn(n); }
    e.preventDefault();
  } else if (e.code === 'KeyZ') { octave = Math.max(-1, octave - 1); updateKbdInfo(); }
  else if (e.code === 'KeyX') { octave = Math.min(8, octave + 1); updateKbdInfo(); }
  else if (e.code === 'KeyC') { velocity = Math.max(16, velocity - 16); updateKbdInfo(); }
  else if (e.code === 'KeyV') { velocity = Math.min(127, velocity + 16); updateKbdInfo(); }
  else if (e.code === 'Space') { e.preventDefault(); transport(last.playing ? 'stop' : 'play'); }
  else if (e.code === 'Escape') transport('stop');
  else if (e.code === 'Digit1') transport('record');
});
window.addEventListener('keyup', (e) => {
  if (e.code === 'AltLeft' || e.code === 'AltRight') { call('sustain', { down: false }); return; }
  if (held.has(e.code)) { noteOff(held.get(e.code)); held.delete(e.code); }
});
window.addEventListener('blur', () => { for (const n of held.values()) noteOff(n); held.clear(); call('transport', { action: 'panic' }).catch(() => {}); });

// ---------- piano roll view ----------
const canvas = $('roll');
let clip = { version: -1, length_ticks: 0, notes: [] };
let pos = 0;
function drawRoll() {
  const dpr = window.devicePixelRatio || 1;
  const w = canvas.clientWidth, h = canvas.clientHeight;
  if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) { canvas.width = Math.round(w * dpr); canvas.height = Math.round(h * dpr); }
  const g = canvas.getContext('2d');
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  const css = getComputedStyle(document.documentElement);
  g.clearRect(0, 0, w, h);
  const barTicks = PPQ * 4 * (Number($('num').value) || 4) / (Number($('den').value) || 4);
  const span = Math.max(clip.length_ticks, barTicks * 2);
  const notes = clip.notes;
  let lo = 48, hi = 72;
  if (notes.length) { lo = Math.min(...notes.map((n) => n.note)) - 2; hi = Math.max(...notes.map((n) => n.note)) + 2; }
  if (hi - lo < 12) { const c = (hi + lo) / 2; lo = Math.floor(c - 6); hi = Math.ceil(c + 6); }
  const rows = hi - lo + 1, rh = h / rows, x = (t) => (t / span) * w;
  g.strokeStyle = css.getPropertyValue('--grid');
  g.lineWidth = 1;
  for (let t = 0; t <= span; t += barTicks / (Number($('num').value) || 4)) {
    g.globalAlpha = t % barTicks === 0 ? 1 : 0.4;
    g.beginPath(); g.moveTo(x(t) + 0.5, 0); g.lineTo(x(t) + 0.5, h); g.stroke();
  }
  g.globalAlpha = 1;
  for (let n = lo; n <= hi; n++) if (isBlack(n)) { g.fillStyle = 'rgba(128,128,128,0.08)'; g.fillRect(0, h - (n - lo + 1) * rh, w, rh); }
  g.fillStyle = css.getPropertyValue('--note');
  for (const n of notes) {
    g.globalAlpha = 0.45 + 0.55 * (n.velocity / 127);
    g.fillRect(x(n.start_tick), h - (n.note - lo + 1) * rh + 1, Math.max(2, x(n.end_tick) - x(n.start_tick)), rh - 2);
  }
  g.globalAlpha = 1;
  if (last.playing && pos >= 0) {
    g.strokeStyle = css.getPropertyValue('--playhead'); g.lineWidth = 2;
    const px = Math.min(w - 1, x(pos));
    g.beginPath(); g.moveTo(px, 0); g.lineTo(px, h); g.stroke();
  }
  if (!notes.length) {
    g.fillStyle = css.getPropertyValue('--muted'); g.font = '14px sans-serif'; g.textAlign = 'center';
    g.fillText(last.recording ? 'Enregistrement…' : 'Aucune prise. Clique sur « Enregistrer » et joue.', w / 2, h / 2);
  }
}
async function refreshClip() {
  clip = await call('get_clip');
  const bars = clip.length_ticks / (PPQ * 4 * (Number($('num').value) || 4) / (Number($('den').value) || 4));
  $('clip-info').textContent = clip.notes.length ? `· ${clip.notes.length} notes, ${bars} mesures` : '';
}

// ---------- polling ----------
let busy = false;
let lastClipVersion = -1;
let lastMidiCount = 0;
let meterL = 0, meterR = 0;
async function tick() {
  if (busy || !invoke) return;
  busy = true;
  try {
    const s = await call('poll');
    last = s;
    pos = s.position_ticks;
    const beatTicks = PPQ * 4 / s.denominator, barTicks = beatTicks * s.numerator;
    const time = $('time');
    if (s.playing && pos < 0) { time.textContent = 'Décompte ' + Math.ceil(-pos / beatTicks); time.classList.add('countin'); }
    else { time.classList.remove('countin'); const p = Math.max(0, pos); time.textContent = `${Math.floor(p / barTicks) + 1} . ${Math.floor((p % barTicks) / beatTicks) + 1}`; }
    $('btn-rec').classList.toggle('on', s.recording);
    $('btn-play').classList.toggle('on', s.playing && !s.recording);
    meterL = Math.max(dbToPct(s.peak_l), meterL * 0.85); meterR = Math.max(dbToPct(s.peak_r), meterR * 0.85);
    $('mL').style.width = meterL * 100 + '%'; $('mR').style.width = meterR * 100 + '%';
    $('load').textContent = Math.round(s.load * 100);
    $('voices').textContent = s.voices;
    const m = s.midi;
    setStatus($('midi-status'), m.inputs.length ? 'Connecté : ' + m.inputs.join(', ') : 'Aucun clavier MIDI détecté (branche-le en USB).');
    $('midi-last').textContent = m.last_message ? 'Dernier message : ' + m.last_message : '';
    $('midi-led').classList.toggle('on', m.message_count !== lastMidiCount);
    lastMidiCount = m.message_count;
    if (s.stream_errors > 0) setStatus($('audio-status'), `${s.stream_errors} erreur(s) du pilote audio — essaie un tampon plus grand.`, true);
    if (s.clip_version !== lastClipVersion) { lastClipVersion = s.clip_version; await refreshClip(); }
    drawRoll();
  } catch (e) { /* engine not ready yet */ }
  busy = false;
}

// ---------- init ----------
(async function init() {
  buildPiano();
  updateKbdInfo();
  if (!invoke) { setStatus($('audio-status'), 'Lance l\'application avec « cargo tauri dev ».', true); drawRoll(); return; }
  try {
    await loadDevices();
    await buildSynth();
    await applyAudio();
  } catch (e) { setStatus($('audio-status'), String(e), true); }
  setInterval(tick, 33);
})();
window.addEventListener('resize', drawRoll);
