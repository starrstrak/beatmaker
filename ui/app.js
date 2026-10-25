'use strict';
// BeatMaker UI (phase 2). Plain JS on purpose: no build step.
// All sound runs in the Rust engine; this file only sends commands and draws state.

const invoke = window.__TAURI__ && window.__TAURI__.core ? window.__TAURI__.core.invoke : null;
const $ = (id) => document.getElementById(id);
const PPQ = 960;

// ---------------------------------------------------------------- UI text (French)
const INSTR = {
  subtractive: ['Synthé soustractif', 'Son riche et filtré : basses, leads, nappes, pizzicati.'],
  fm: ['Synthé FM', 'Sons métalliques ou « électriques » : piano électrique, cloches, basses FM.'],
  drums: ['Boîte à rythmes', 'Batterie synthétisée : grosse caisse (do 2), caisse claire (ré 2), charleston (fa♯ 2)…'],
};
const FX = {
  filter: ['Filtre', 'Coupe les aigus (passe-bas), les graves (passe-haut) ou garde une bande.'],
  drive: ['Distorsion', 'Sature le son : chaleur, mordant, agressivité.'],
  chorus: ['Chorus', 'Épaissit et élargit le son en le doublant légèrement désaccordé.'],
  delay: ['Delay (écho)', 'Répète le son en s\'estompant. Idéal pour les leads et les voix.'],
  reverb: ['Reverb', 'Simule une pièce ou une salle : espace et profondeur.'],
  compressor: ['Compresseur', 'Réduit les écarts de volume pour un son plus dense et plus présent.'],
};
const GROUPS = {
  osc: 'Oscillateurs', filter: 'Filtre', amp: 'Volume (enveloppe)', fenv: 'Enveloppe du filtre', perf: 'Jeu',
  out: 'Sortie', fm: 'Modulation FM', kick: 'Grosse caisse', snare: 'Caisse claire', hats: 'Charleston', toms: 'Toms',
  drive: 'Distorsion', chorus: 'Chorus', delay: 'Delay', reverb: 'Reverb', comp: 'Compresseur',
};
const WAVES = ['Sinus', 'Triangle', 'Dent de scie', 'Carré'];
const CHOICES = {
  'subtractive.osc1_wave': WAVES, 'subtractive.osc2_wave': WAVES,
  'filter.type': ['Passe-bas', 'Passe-haut', 'Passe-bande'],
  'delay.pingpong': ['Mono', 'Ping-pong'],
};
const PT = { // [label, tooltip]
  subtractive: {
    osc1_wave: ['Forme d\'onde', 'Le timbre de base : sinus = doux, triangle = rond, dent de scie = riche et brillant, carré = creux.'],
    osc2_wave: ['Forme d\'onde 2', 'Forme du deuxième oscillateur.'],
    osc2_detune: ['Désaccord', 'Désaccorde le 2e oscillateur : son plus épais.'],
    osc2_level: ['Niveau osc. 2', 'Quantité du 2e oscillateur dans le mélange.'],
    cutoff: ['Coupure', 'Plus c\'est bas, plus le son est sourd ; plus c\'est haut, plus il est brillant.'],
    resonance: ['Résonance', 'Accentue les fréquences autour de la coupure : son plus « vocal ».'],
    filter_env: ['Env. filtre', 'De combien l\'enveloppe ouvre le filtre à chaque note (en octaves).'],
    amp_attack: ['Attaque', 'Temps pour atteindre le volume max. Court = percussif, long = progressif.'],
    amp_decay: ['Déclin', 'Temps pour retomber vers le niveau de maintien.'],
    amp_sustain: ['Maintien', 'Niveau tenu tant que la touche reste enfoncée.'],
    amp_release: ['Relâche', 'Durée d\'extinction du son après avoir lâché la touche.'],
    flt_attack: ['Attaque', 'Vitesse d\'ouverture du filtre.'], flt_decay: ['Déclin', 'Vitesse de retombée du filtre.'],
    flt_sustain: ['Maintien', 'Niveau du filtre tant que la touche est tenue.'], flt_release: ['Relâche', 'Vitesse de fermeture du filtre.'],
    bend_range: ['Pitch bend', 'Étendue de la molette de hauteur, en demi-tons.'],
    mod_depth: ['Vibrato', 'Profondeur du vibrato commandé par la molette de modulation et l\'aftertouch.'],
    level: ['Volume', 'Volume de l\'instrument.'],
  },
  fm: {
    ratio: ['Rapport', 'Rapport de fréquence entre modulateur et porteuse : entier = harmonique, décimal = métallique.'],
    index: ['Indice', 'Intensité de la modulation : plus c\'est haut, plus le son est brillant et riche.'],
    index_env: ['Env. indice', 'Part de l\'indice qui s\'éteint avec le temps (attaque brillante puis son rond).'],
    index_decay: ['Déclin indice', 'Durée de cette extinction.'],
    feedback: ['Feedback', 'Réinjecte le modulateur dans lui-même : plus rugueux.'],
    amp_attack: ['Attaque', 'Temps pour atteindre le volume max.'], amp_decay: ['Déclin', 'Temps pour retomber vers le maintien.'],
    amp_sustain: ['Maintien', 'Niveau tenu pendant la note.'], amp_release: ['Relâche', 'Durée d\'extinction après la note.'],
    bend_range: ['Pitch bend', 'Étendue de la molette de hauteur.'], mod_depth: ['Vibrato', 'Vibrato commandé par la molette de modulation.'],
    level: ['Volume', 'Volume de l\'instrument.'],
  },
  drums: {
    kick_tune: ['Hauteur', 'Hauteur de la grosse caisse.'], kick_decay: ['Longueur', 'Durée de la grosse caisse : courte = sèche, longue = 808.'],
    snare_tone: ['Corps', 'Hauteur du corps de la caisse claire.'], snare_snap: ['Claquant', 'Part de bruit (timbre) de la caisse claire.'],
    hat_decay: ['Longueur', 'Durée du charleston.'], tom_tune: ['Hauteur', 'Accord des toms.'], level: ['Volume', 'Volume de la batterie.'],
  },
  filter: {
    type: ['Type', 'Passe-bas, passe-haut ou passe-bande.'], cutoff: ['Coupure', 'Fréquence de coupure du filtre.'],
    resonance: ['Résonance', 'Accentue la fréquence de coupure.'],
  },
  drive: {
    drive: ['Drive', 'Quantité de saturation.'], tone: ['Tonalité', 'Adoucit les aigus après la saturation.'],
    mix: ['Mix', 'Équilibre son original / son saturé.'], level: ['Niveau', 'Volume de sortie.'],
  },
  chorus: {
    rate: ['Vitesse', 'Vitesse de l\'ondulation.'], depth: ['Profondeur', 'Amplitude de l\'effet.'], mix: ['Mix', 'Quantité d\'effet.'],
  },
  delay: {
    time: ['Temps', 'Délai entre les répétitions.'], feedback: ['Répétitions', 'Nombre de répétitions audibles.'],
    tone: ['Tonalité', 'Assombrit chaque répétition.'], pingpong: ['Mode', 'Ping-pong : les échos rebondissent gauche/droite.'],
    mix: ['Mix', 'Quantité d\'écho.'],
  },
  reverb: {
    size: ['Taille', 'Taille de la pièce.'], damp: ['Amorti', 'Absorbe les aigus : salle feutrée.'],
    width: ['Largeur', 'Largeur stéréo.'], mix: ['Mix', 'Quantité de reverb.'],
  },
  compressor: {
    threshold: ['Seuil', 'Niveau à partir duquel la compression agit.'], ratio: ['Ratio', 'Force de la compression.'],
    attack: ['Attaque', 'Vitesse de réaction.'], release: ['Relâche', 'Vitesse de retour.'],
    makeup: ['Gain', 'Remonte le volume après compression.'], mix: ['Mix', 'Compression parallèle.'],
  },
};
const LFO_TARGETS = [['none', 'Aucune'], ['pitch', 'Hauteur (vibrato)'], ['brightness', 'Brillance'], ['volume', 'Volume (trémolo)'], ['pan', 'Panoramique']];
const LFO_SHAPES = [['sine', 'Sinus'], ['triangle', 'Triangle'], ['square', 'Carré'], ['samplehold', 'Aléatoire']];

// ---------------------------------------------------------------- helpers
const store = {
  get(k, d) { try { return localStorage.getItem(k) ?? d; } catch { return d; } },
  set(k, v) { try { localStorage.setItem(k, v); } catch { /* storage unavailable */ } },
};
async function call(cmd, args) {
  if (!invoke) throw new Error('Lance l\'application avec « cargo tauri dev » (pas dans un navigateur).');
  return invoke(cmd, args || {});
}
let toastTimer = null;
function toast(msg, err) {
  const el = $('toast');
  el.textContent = msg; el.classList.toggle('err', !!err); el.hidden = false;
  clearTimeout(toastTimer); toastTimer = setTimeout(() => { el.hidden = true; }, err ? 6000 : 2800);
}
const safe = (p) => p.catch((e) => toast(String(e), true));
function setStatus(el, text, err) { el.textContent = text; el.classList.toggle('err', !!err); }
const dbToPct = (v) => (v <= 0.0001 ? 0 : Math.max(0, Math.min(1, (20 * Math.log10(v) + 60) / 60)));
const el = (tag, cls, html) => { const e = document.createElement(tag); if (cls) e.className = cls; if (html != null) e.innerHTML = html; return e; };
const typing = (e) => /^(INPUT|SELECT|TEXTAREA)$/.test(e.target.tagName) && e.target.type !== 'range' && e.target.type !== 'checkbox';
const colorVar = (i) => `var(--c${i % 8})`;

// ---------------------------------------------------------------- mode & theme
function applyMode(m) {
  document.documentElement.dataset.mode = m;
  $('mode-beginner').classList.toggle('active', m === 'beginner');
  $('mode-pro').classList.toggle('active', m === 'pro');
  store.set('mode', m);
  if (catalog) renderAll();
}
$('mode-beginner').onclick = () => applyMode('beginner');
$('mode-pro').onclick = () => applyMode('pro');
document.documentElement.dataset.theme = store.get('theme', 'dark');
$('theme').onclick = () => {
  const t = document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark';
  document.documentElement.dataset.theme = t; store.set('theme', t); drawAll();
};

// ---------------------------------------------------------------- state
let catalog = null;
let tracks = [];
let selected = null;
let lastVersion = -1;
let st = { playing: false, recording: false, numerator: 4, denominator: 4, loop: false, loop_bars: 4, position_ticks: 0 };
const meters = new Map();

const sel = () => tracks.find((t) => t.id === selected) || tracks[0] || null;
const barTicks = () => PPQ * 4 * st.numerator / st.denominator;
function spanTicks() {
  const longest = Math.max(0, ...tracks.map((t) => t.clip_length));
  return Math.max(barTicks() * 4, st.loop ? barTicks() * st.loop_bars : 0, longest);
}

// ---------------------------------------------------------------- knob widget
const toNorm = (p, v) => (p.curve === 'Log' ? Math.log(v / p.min) / Math.log(p.max / p.min) : (v - p.min) / (p.max - p.min));
const fromNorm = (p, n) => (p.curve === 'Log' ? p.min * Math.pow(p.max / p.min, n) : p.min + n * (p.max - p.min));
function fmt(p, v) {
  switch (p.unit) {
    case 'Hz': return v >= 1000 ? (v / 1000).toFixed(1) + ' kHz' : Math.round(v) + ' Hz';
    case 's': return v < 1 ? Math.round(v * 1000) + ' ms' : v.toFixed(2) + ' s';
    case 'ms': return v < 100 ? v.toFixed(1) + ' ms' : Math.round(v) + ' ms';
    case 'ct': return Math.round(v) + ' ct';
    case 'oct': return v.toFixed(1) + ' oct';
    case 'st': return v.toFixed(1) + ' st';
    case 'dB': return v.toFixed(1) + ' dB';
    case ':1': return v.toFixed(1) + ':1';
    case 'x': return '×' + v.toFixed(2);
    default: return Math.round(v * 100) + ' %';
  }
}
const polar = (cx, cy, r, deg) => [cx + r * Math.cos((deg * Math.PI) / 180), cy + r * Math.sin((deg * Math.PI) / 180)];
function arcPath(r, a0, a1) {
  const [x0, y0] = polar(22, 22, r, a0), [x1, y1] = polar(22, 22, r, a1);
  return `M${x0.toFixed(2)} ${y0.toFixed(2)} A${r} ${r} 0 ${a1 - a0 > 180 ? 1 : 0} 1 ${x1.toFixed(2)} ${y1.toFixed(2)}`;
}
/** Rotary knob: drag vertically (Shift = fine), wheel, double-click = default. */
function makeKnob(info, label, tip, value, onChange, accent) {
  const root = el('div', 'knob');
  root.title = tip + '\n(Glisser = régler · Maj = précis · Double-clic = valeur par défaut)';
  const NS = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(NS, 'svg'); svg.setAttribute('viewBox', '0 0 44 44');
  const mk = (tag, attrs) => { const n = document.createElementNS(NS, tag); for (const k in attrs) n.setAttribute(k, attrs[k]); svg.append(n); return n; };
  mk('circle', { cx: 22, cy: 22, r: 15, fill: 'var(--knob)', stroke: 'var(--knob2)', 'stroke-width': 2 });
  mk('path', { d: arcPath(19.5, 135, 405), fill: 'none', stroke: 'var(--knob2)', 'stroke-width': 3, 'stroke-linecap': 'round' });
  const val = mk('path', { fill: 'none', stroke: accent, 'stroke-width': 3, 'stroke-linecap': 'round' });
  const ptr = mk('line', { stroke: 'var(--text)', 'stroke-width': 2.2, 'stroke-linecap': 'round' });
  const lab = el('div', 'kl', label), out = el('div', 'kv');
  root.append(svg, lab, out);
  let norm = toNorm(info, value);
  const draw = () => {
    const a = 135 + norm * 270;
    val.setAttribute('d', norm > 0.001 ? arcPath(19.5, 135, a) : '');
    const [x0, y0] = polar(22, 22, 6, a), [x1, y1] = polar(22, 22, 13, a);
    ptr.setAttribute('x1', x0); ptr.setAttribute('y1', y0); ptr.setAttribute('x2', x1); ptr.setAttribute('y2', y1);
    out.textContent = fmt(info, fromNorm(info, norm));
  };
  const set = (n, emit = true) => { norm = Math.min(1, Math.max(0, n)); draw(); if (emit) onChange(fromNorm(info, norm)); };
  draw();
  root.onpointerdown = (e) => {
    root.setPointerCapture(e.pointerId);
    const y0 = e.clientY, n0 = norm;
    const move = (m) => set(n0 + (y0 - m.clientY) / (m.shiftKey ? 700 : 170));
    const up = () => { root.removeEventListener('pointermove', move); root.removeEventListener('pointerup', up); };
    root.addEventListener('pointermove', move); root.addEventListener('pointerup', up);
  };
  root.ondblclick = () => set(toNorm(info, info.default));
  root.onwheel = (e) => { e.preventDefault(); set(norm - Math.sign(e.deltaY) * 0.02); };
  return root;
}
function makeChoice(labels, label, tip, value, onChange) {
  const box = el('div', 'cbox'); box.title = tip;
  const s = el('select');
  labels.forEach((l, i) => s.add(new Option(l, i)));
  s.value = Math.round(value);
  s.onchange = () => onChange(Number(s.value));
  box.append(s, el('div', 'kl', label));
  return box;
}
function paramControls(device, infos, values, accent, send) {
  const wrap = el('div', 'dbody');
  const groups = [];
  infos.forEach((p, i) => { let g = groups.find((x) => x.name === p.group); if (!g) groups.push((g = { name: p.group, items: [] })); g.items.push([p, i]); });
  const pro = document.documentElement.dataset.mode === 'pro';
  for (const g of groups) {
    const items = g.items.filter(([p]) => pro || p.basic);
    if (!items.length) continue;
    wrap.append(el('div', 'glabel', GROUPS[g.name] || g.name));
    const row = el('div', 'knobs');
    for (const [p, i] of items) {
      const [label, tip] = (PT[device] && PT[device][p.key]) || [p.key, ''];
      const choices = CHOICES[device + '.' + p.key];
      row.append(choices
        ? makeChoice(choices, label, tip, values[i], (v) => { values[i] = v; send(i, v); })
        : makeKnob(p, label, tip, values[i], (v) => { values[i] = v; send(i, v); }, accent));
    }
    wrap.append(row);
  }
  return wrap;
}

// ---------------------------------------------------------------- sequencer lanes
const lanesEl = $('lanes');
const canvases = new Map(); // track id -> canvas
let rulerCanvas = null;
let playheadEl = null;
const meterEls = new Map();

function buildLane(t) {
  const lane = el('div', 'lane' + (t.id === sel()?.id ? ' sel' : ''));
  lane.style.setProperty('--lc', colorVar(t.color));
  const head = el('div', 'lane-head');
  const r1 = el('div', 'row');
  const name = el('input'); name.type = 'text'; name.value = t.name; name.title = 'Nom de la piste (clique pour renommer)';
  name.onchange = () => safe(call('rename_track', { id: t.id, name: name.value }));
  name.onfocus = () => selectTrack(t.id);
  const arm = el('button', 'tb arm' + (t.armed ? ' on' : ''), '●'); arm.title = 'Armer : le clavier MIDI joue cet instrument et l\'enregistrement va sur cette piste';
  arm.onclick = (e) => { e.stopPropagation(); safe(call('arm_track', { id: t.id })); };
  const mute = el('button', 'tb mute' + (t.mute ? ' on' : ''), 'M'); mute.title = 'Muet';
  mute.onclick = (e) => { e.stopPropagation(); safe(call('set_mix', { id: t.id, mute: !t.mute })); };
  const solo = el('button', 'tb solo' + (t.solo ? ' on' : ''), 'S'); solo.title = 'Solo : n\'écouter que cette piste';
  solo.onclick = (e) => { e.stopPropagation(); safe(call('set_mix', { id: t.id, solo: !t.solo })); };
  const del = el('button', 'tb del', '✕'); del.title = 'Supprimer la piste';
  del.onclick = (e) => { e.stopPropagation(); if (confirm(`Supprimer « ${t.name} » ?`)) safe(call('remove_track', { id: t.id })); };
  r1.append(name, arm, mute, solo, del);

  const r2 = el('div', 'row');
  const inst = el('select'); inst.title = 'Instrument de cette piste';
  for (const k in INSTR) inst.add(new Option(INSTR[k][0], k));
  inst.value = t.instrument;
  inst.onchange = () => safe(call('set_instrument', { id: t.id, kind: inst.value }));
  const pre = el('select'); pre.title = 'Son prédéfini (preset)';
  pre.add(new Option('— personnalisé —', ''));
  catalog.instruments.find((i) => i.key === t.instrument).presets.forEach((p) => pre.add(new Option(p, p)));
  pre.value = t.preset || '';
  pre.onchange = () => { if (pre.value) safe(call('apply_preset', { id: t.id, name: pre.value })); };
  r2.append(inst, pre);

  const r3 = el('div', 'mini');
  const vol = el('input'); vol.type = 'range'; vol.min = 0; vol.max = 1000; vol.value = Math.round((t.volume / 1.5) * 1000); vol.title = 'Volume de la piste';
  vol.oninput = () => { t.volume = (vol.value / 1000) * 1.5; call('set_mix', { id: t.id, volume: t.volume }); };
  const pan = el('input'); pan.type = 'range'; pan.min = -100; pan.max = 100; pan.value = Math.round(t.pan * 100); pan.title = 'Panoramique (gauche / droite) · double-clic = centre';
  pan.oninput = () => { t.pan = pan.value / 100; call('set_mix', { id: t.id, pan: t.pan }); };
  pan.ondblclick = () => { pan.value = 0; pan.oninput(); };
  r3.append('Vol', vol, 'Pan', pan);
  const lm = el('div', 'lmeter'); const lmi = el('div'); lm.append(lmi); meterEls.set(t.id, lmi);

  // Quantise: stepped slider from 1/1 down to 1/128.
  const q = el('div', 'qrow');
  const divs = catalog.divisions;
  const qs = el('input'); qs.type = 'range'; qs.min = 0; qs.max = divs.length - 1; qs.step = 1;
  qs.value = Math.max(0, divs.indexOf(t.quantize.division));
  qs.title = 'Palier de quantification : de 1/1 (une ronde) à 1/128 (la plus fine)';
  const ql = el('span', 'qlabel');
  const strength = el('input'); strength.type = 'range'; strength.min = 0; strength.max = 100; strength.value = Math.round(t.quantize.strength * 100); strength.title = 'Force : 100 % = pile sur la grille, 0 % = aucun changement';
  const swing = el('input'); swing.type = 'range'; swing.min = 50; swing.max = 75; swing.value = Math.round(t.quantize.swing * 100); swing.title = 'Swing : 50 % = droit, 66 % = ternaire';
  const trip = el('input'); trip.type = 'checkbox'; trip.checked = t.quantize.triplet; trip.title = 'Grille en triolets';
  const qargs = () => ({ id: t.id, division: divs[Number(qs.value)], triplet: trip.checked, strength: strength.value / 100, swing: swing.value / 100 });
  const refreshLabel = () => { ql.textContent = '1/' + divs[Number(qs.value)] + (trip.checked ? 'T' : ''); };
  refreshLabel();
  const sendQ = () => { refreshLabel(); safe(call('set_quantize', qargs())); };
  qs.oninput = sendQ; strength.oninput = sendQ; swing.oninput = sendQ; trip.onchange = sendQ;
  const qb = el('button', 'primary small', 'Quantifier'); qb.title = 'Aligne les notes de cette piste sur la grille choisie (annulable)';
  qb.onclick = async (e) => {
    e.stopPropagation();
    try { await call('set_quantize', qargs()); await call('quantize_track', { id: t.id }); toast(`Piste « ${t.name} » quantifiée en ${ql.textContent}`); }
    catch (err) { toast(String(err), true); }
  };
  const proBox = el('span', 'pro');
  proBox.append('Force', strength, 'Swing', swing, el('label', 'check', '<span>Triolet</span>'));
  proBox.lastChild.prepend(trip);
  const clr = el('button', 'small pro', 'Vider'); clr.title = 'Efface la prise de cette piste (annulable)';
  clr.onclick = () => safe(call('clear_clip', { id: t.id }));
  q.append(el('span', 'muted', 'Palier'), qs, ql, qb, proBox, clr);

  head.append(r1, r2, r3, lm, q);
  head.onclick = () => selectTrack(t.id);

  const body = el('div', 'lane-body');
  const cv = el('canvas');
  body.append(cv); canvases.set(t.id, cv);
  lane.append(head, body);
  return lane;
}

function renderLanes() {
  canvases.clear(); meterEls.clear();
  lanesEl.innerHTML = '';
  const ruler = el('div', 'ruler');
  ruler.append(el('div'), (rulerCanvas = el('canvas')));
  lanesEl.append(ruler);
  for (const t of tracks) lanesEl.append(buildLane(t));
  playheadEl = el('div', 'playhead');
  lanesEl.append(playheadEl);
  requestAnimationFrame(drawAll);
}

function sizeCanvas(cv) {
  const dpr = window.devicePixelRatio || 1;
  const w = cv.clientWidth, h = cv.clientHeight;
  if (cv.width !== Math.round(w * dpr) || cv.height !== Math.round(h * dpr)) { cv.width = Math.round(w * dpr); cv.height = Math.round(h * dpr); }
  const g = cv.getContext('2d'); g.setTransform(dpr, 0, 0, dpr, 0, 0); g.clearRect(0, 0, w, h);
  return [g, w, h];
}
function drawGrid(g, w, h, span, full) {
  const css = getComputedStyle(document.documentElement);
  const bt = barTicks(), beat = (PPQ * 4) / st.denominator;
  const xOf = (t) => (t / span) * w;
  if (full && st.loop) { g.fillStyle = 'rgba(61,214,198,0.07)'; g.fillRect(0, 0, xOf(bt * st.loop_bars), h); }
  g.lineWidth = 1;
  for (let t = 0; t <= span; t += beat) {
    const isBar = Math.round(t) % Math.round(bt) === 0;
    if (!isBar && xOf(beat) < 14) continue;
    g.strokeStyle = css.getPropertyValue(isBar ? '--grid' : '--grid2');
    g.beginPath(); g.moveTo(Math.round(xOf(t)) + 0.5, 0); g.lineTo(Math.round(xOf(t)) + 0.5, h); g.stroke();
  }
}
function drawLane(t) {
  const cv = canvases.get(t.id); if (!cv) return;
  const [g, w, h] = sizeCanvas(cv);
  const span = spanTicks();
  drawGrid(g, w, h, span, true);
  const x = (tick) => (tick / span) * w;
  if (t.notes.length) {
    let lo = Math.min(...t.notes.map((n) => n.note)) - 1, hi = Math.max(...t.notes.map((n) => n.note)) + 1;
    if (hi - lo < 10) { const c = (hi + lo) / 2; lo = Math.floor(c - 5); hi = Math.ceil(c + 5); }
    const rows = hi - lo + 1, rh = h / rows;
    const css = getComputedStyle(document.documentElement);
    const color = css.getPropertyValue('--c' + (t.color % 8)).trim();
    g.fillStyle = color;
    for (const n of t.notes) {
      g.globalAlpha = 0.45 + 0.55 * (n.velocity / 127);
      g.fillRect(x(n.start_tick), h - (n.note - lo + 1) * rh + 1, Math.max(2, x(n.end_tick) - x(n.start_tick)), Math.max(2, rh - 2));
    }
    g.globalAlpha = 1;
    g.strokeStyle = color; g.globalAlpha = 0.5; g.strokeRect(0.5, 0.5, Math.max(1, x(t.clip_length) - 1), h - 1); g.globalAlpha = 1;
  } else {
    g.fillStyle = getComputedStyle(document.documentElement).getPropertyValue('--muted');
    g.font = '12px sans-serif'; g.textAlign = 'left';
    g.fillText(t.armed ? 'Piste armée : appuie sur « Enregistrer » et joue.' : 'Prise vide', 10, h / 2);
  }
}
function drawRuler() {
  if (!rulerCanvas) return;
  const [g, w, h] = sizeCanvas(rulerCanvas);
  const span = spanTicks(), bt = barTicks();
  g.fillStyle = getComputedStyle(document.documentElement).getPropertyValue('--muted');
  g.font = '11px sans-serif';
  for (let b = 0; b * bt < span; b++) { g.fillText(String(b + 1), (b * bt / span) * w + 4, 14); }
  drawGrid(g, w, h, span, false);
}
function drawAll() { drawRuler(); tracks.forEach(drawLane); }
window.addEventListener('resize', drawAll);

function selectTrack(id) {
  if (selected === id) return;
  selected = id;
  renderLanes(); renderRack();
}

// ---------------------------------------------------------------- rack
function deviceBox(title, accent, extra) {
  const d = el('div', 'device'); d.style.setProperty('--dc', accent);
  const head = el('div', 'dhead'); head.append(el('span', 't', title));
  if (extra) extra.forEach((x) => head.append(x));
  d.append(head);
  return [d, head];
}
function renderRack() {
  const t = sel(), rack = $('rack'), chain = $('chain');
  rack.innerHTML = ''; chain.innerHTML = '';
  if (!t) { $('rack-title').textContent = 'Rack'; return; }
  $('rack-title').textContent = `Rack · ${t.name}`;
  const accent = colorVar(t.color);
  const cat = catalog.instruments.find((i) => i.key === t.instrument);

  // Signal chain overview (click a chip to scroll to the device).
  const chip = (label, id, off) => { const c = el('span', 'chip' + (off ? ' off' : ''), label); c.onclick = () => document.getElementById(id)?.scrollIntoView({ behavior: 'smooth', block: 'nearest' }); return c; };
  chain.append(chip(INSTR[t.instrument][0], 'dev-inst'), el('i', '', '→'), chip('Modulation', 'dev-lfo'));
  t.effects.forEach((e, i) => chain.append(el('i', '', '→'), chip(FX[e.kind][0], 'dev-fx' + i, e.bypass)));
  chain.append(el('i', '', '→'), el('span', 'chip', 'Mixeur'), el('i', '', '→'), el('span', 'chip', 'Sortie'));

  // Instrument
  const [dInst] = deviceBox(INSTR[t.instrument][0], accent);
  dInst.id = 'dev-inst'; dInst.title = INSTR[t.instrument][1];
  const dpre = el('select'); dpre.add(new Option('— personnalisé —', ''));
  cat.presets.forEach((p) => dpre.add(new Option(p, p))); dpre.value = t.preset || '';
  dpre.onchange = () => { if (dpre.value) safe(call('apply_preset', { id: t.id, name: dpre.value })); };
  dInst.firstChild.append(dpre);
  dInst.append(paramControls(t.instrument, cat.params, t.params, accent, (index, value) => { call('set_instrument_param', { id: t.id, index, value }); dpre.value = ''; }));
  rack.append(dInst);

  // Modulation (LFO)
  const [dLfo] = deviceBox('Modulation (LFO)', 'var(--c3)');
  dLfo.id = 'dev-lfo'; dLfo.title = 'Fait osciller un réglage de la piste en continu : vibrato, trémolo, auto-pan, filtre qui respire…';
  const body = el('div', 'dbody');
  const sendLfo = () => { call('set_lfo', { id: t.id, shape: t.lfo.shape, target: t.lfo.target, rateHz: t.lfo.rate_hz, depth: t.lfo.depth }); };
  const mkSel = (opts, val, tip, label, set) => {
    const box = el('div', 'cbox'); box.title = tip; const s = el('select');
    opts.forEach(([v, l]) => s.add(new Option(l, v))); s.value = val;
    s.onchange = () => { set(s.value); sendLfo(); }; box.append(s, el('div', 'kl', label)); return box;
  };
  body.append(mkSel(LFO_TARGETS, t.lfo.target, 'Ce que l\'oscillateur fait varier.', 'Cible', (v) => { t.lfo.target = v; }));
  const shape = mkSel(LFO_SHAPES, t.lfo.shape, 'Forme de l\'oscillation.', 'Forme', (v) => { t.lfo.shape = v; }); shape.classList.add('pro'); body.append(shape);
  const rate = makeKnob({ min: 0.05, max: 20, default: 4, curve: 'Log', unit: 'Hz' }, 'Vitesse', 'Vitesse de l\'oscillation.', t.lfo.rate_hz, (v) => { t.lfo.rate_hz = v; sendLfo(); }, 'var(--c3)');
  rate.classList.add('pro'); body.append(rate);
  body.append(makeKnob({ min: 0, max: 1, default: 0.5, curve: 'Linear', unit: '' }, 'Intensité', 'Force de la modulation.', t.lfo.depth, (v) => { t.lfo.depth = v; sendLfo(); }, 'var(--c3)'));
  dLfo.append(body);
  rack.append(dLfo);

  // Insert effects
  t.effects.forEach((e, i) => {
    const info = catalog.effects.find((x) => x.key === e.kind);
    const led = el('button', e.bypass ? '' : 'on', e.bypass ? 'OFF' : 'ON'); led.title = 'Activer / désactiver ce plug-in';
    led.onclick = () => safe(call('set_effect_bypass', { id: t.id, index: i, bypass: !e.bypass }).then(() => { e.bypass = !e.bypass; renderRack(); }));
    const up = el('button', '', '↑'); up.title = 'Monter dans la chaîne'; up.onclick = () => i > 0 && safe(call('move_effect', { id: t.id, from: i, to: i - 1 }));
    const dn = el('button', '', '↓'); dn.title = 'Descendre dans la chaîne'; dn.onclick = () => i < t.effects.length - 1 && safe(call('move_effect', { id: t.id, from: i, to: i + 1 }));
    const rm = el('button', '', '✕'); rm.title = 'Retirer ce plug-in'; rm.onclick = () => safe(call('remove_effect', { id: t.id, index: i }));
    const [d] = deviceBox(FX[e.kind][0], 'var(--c' + ((i + 4) % 8) + ')', [led, up, dn, rm]);
    d.id = 'dev-fx' + i; d.title = FX[e.kind][1];
    if (e.bypass) d.classList.add('bypassed');
    d.append(paramControls(e.kind, info.params, e.params, 'var(--accent2)', (index, value) => call('set_effect_param', { id: t.id, slot: i, index, value })));
    rack.append(d);
  });
  if (!t.effects.length) rack.append(el('p', 'hint', 'Aucun plug-in. Clique sur « + Plug-in » pour ajouter reverb, delay, compresseur…'));
}

// ---------------------------------------------------------------- menus
function fillMenu(menu, items, onPick) {
  menu.innerHTML = '';
  items.forEach(([key, [name, tip]]) => {
    const b = el('button', '', `${name}<small>${tip}</small>`);
    b.onclick = () => { menu.hidden = true; onPick(key); };
    menu.append(b);
  });
}
$('add-track').onclick = (e) => { e.stopPropagation(); $('fx-menu').hidden = true; $('add-menu').hidden = !$('add-menu').hidden; };
$('add-fx').onclick = (e) => { e.stopPropagation(); $('add-menu').hidden = true; $('fx-menu').hidden = !$('fx-menu').hidden; };
document.addEventListener('click', () => { $('add-menu').hidden = true; $('fx-menu').hidden = true; });

function renderAll() { renderLanes(); renderRack(); updateArmedName(); }
function updateArmedName() {
  const a = tracks.find((t) => t.armed);
  $('armed-name').textContent = a ? a.name : '—';
}
async function refreshProject() {
  const p = await call('get_project');
  tracks = p.tracks; lastVersion = p.version;
  if (!tracks.find((t) => t.id === selected)) selected = (tracks.find((t) => t.armed) || tracks[0] || {}).id ?? null;
  const armed = tracks.find((t) => t.armed);
  if (armed) { const wantOct = armed.instrument === 'drums' ? 2 : 3; if (armedKind !== armed.instrument) { octave = wantOct; armedKind = armed.instrument; updateKbdInfo(); } }
  renderAll();
}
let armedKind = null;

// ---------------------------------------------------------------- audio setup
let devices = [];
async function loadDevices() {
  devices = await call('list_audio_devices');
  const s = $('dev'); s.innerHTML = '';
  devices.forEach((d, i) => { s.add(new Option(d.name + (d.is_default ? ' (par défaut)' : ''), i)); if (d.is_default) s.value = i; });
  fillRates();
}
function fillRates() {
  const d = devices[Number($('dev').value)], r = $('rate'); r.innerHTML = '';
  if (!d) return;
  d.sample_rates.forEach((x) => r.add(new Option(x.toLocaleString('fr-FR') + ' Hz', x)));
  r.value = d.sample_rates.includes(d.default_sample_rate) ? d.default_sample_rate : d.sample_rates[0];
}
$('dev').onchange = fillRates;
async function applyAudio() {
  const d = devices[Number($('dev').value)];
  try {
    const buf = $('buf').value;
    const info = await call('start_audio', { device: d ? d.name : null, sampleRate: Number($('rate').value) || null, bufferFrames: buf ? Number(buf) : null });
    const lat = info.latency_ms != null ? ` · latence ≈ ${info.latency_ms.toFixed(1)} ms` : '';
    setStatus($('audio-status'), `Audio actif : ${info.device}, ${info.sample_rate} Hz, ${info.channels} canaux${lat}`);
  } catch (e) { setStatus($('audio-status'), String(e), true); }
}
$('apply-audio').onclick = applyAudio;

// ---------------------------------------------------------------- transport & global controls
const transport = (action) => safe(call('transport', { action }));
$('btn-rec').onclick = () => transport('record');
$('btn-play').onclick = () => transport('play');
$('btn-stop').onclick = () => transport('stop');
$('tempo').onchange = (e) => call('set_tempo', { bpm: Number(e.target.value) });
const sendSig = () => call('set_time_signature', { numerator: Number($('num').value), denominator: Number($('den').value) });
$('num').onchange = sendSig; $('den').onchange = sendSig;
$('countin').onchange = (e) => call('set_count_in', { beats: Number(e.target.value) });
$('metro').onchange = (e) => call('set_metronome', { on: e.target.checked });
$('overdub').onchange = (e) => call('set_overdub', { on: e.target.checked });
const sendLoop = () => call('set_loop', { enabled: $('loop').checked, bars: Number($('loopbars').value) });
$('loop').onchange = sendLoop; $('loopbars').onchange = sendLoop;
$('master').oninput = (e) => call('set_master', { gain: (e.target.value / 1000) * 1.5 });
$('undo').onclick = () => safe(call('undo'));
$('redo').onclick = () => safe(call('redo'));
$('btn-export').onclick = async () => {
  const out = $('export-status'); setStatus(out, 'Rendu en cours…');
  try {
    const r = await call('export_wav', { sampleRate: Number($('xrate').value), bits: Number($('xbits').value), dither: $('xdither').checked });
    setStatus(out, `Exporté (${r.seconds.toFixed(1)} s, crête ${(20 * Math.log10(Math.max(r.peak, 1e-6))).toFixed(1)} dBFS) : ${r.path}`);
  } catch (e) { setStatus(out, String(e), true); }
};

// ---------------------------------------------------------------- keyboards
let octave = 3, velocity = 100;
const KEYMAP = { KeyA: 0, KeyW: 1, KeyS: 2, KeyE: 3, KeyD: 4, KeyF: 5, KeyT: 6, KeyG: 7, KeyY: 8, KeyH: 9, KeyU: 10, KeyJ: 11, KeyK: 12, KeyO: 13, KeyL: 14, KeyP: 15, Semicolon: 16 };
const NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];
const isBlack = (n) => [1, 3, 6, 8, 10].includes(n % 12);
const noteName = (n) => NAMES[n % 12] + (Math.floor(n / 12) - 1);
const held = new Map(), keyEls = new Map();
function noteOn(n) { call('note', { note: n, velocity, on: true }).catch(() => {}); keyEls.get(n)?.classList.add('down'); }
function noteOff(n) { call('note', { note: n, velocity: 0, on: false }).catch(() => {}); keyEls.get(n)?.classList.remove('down'); }
function buildPiano() {
  const root = $('piano'); root.innerHTML = '';
  const first = 24, last = 96, whites = [];
  for (let n = first; n <= last; n++) if (!isBlack(n)) whites.push(n);
  const w = 100 / whites.length; let wi = 0;
  for (let n = first; n <= last; n++) {
    const k = el('div');
    if (isBlack(n)) { k.className = 'pk b'; k.style.left = (wi * w - w * 0.3) + '%'; k.style.width = (w * 0.6) + '%'; }
    else { k.className = 'pk w'; k.style.left = (wi * w) + '%'; k.style.width = w + '%'; if (n % 12 === 0) k.innerHTML = `<span>${noteName(n)}</span>`; wi++; }
    k.onpointerdown = (e) => { e.preventDefault(); k.setPointerCapture(e.pointerId); held.set('p' + n, n); noteOn(n); };
    k.onpointerup = k.onpointercancel = () => { if (held.delete('p' + n)) noteOff(n); };
    keyEls.set(n, k); root.append(k);
  }
}
function updateKbdInfo() { $('oct').textContent = noteName(12 * (octave + 1)); $('vel').textContent = velocity; }
let last = { playing: false };
window.addEventListener('keydown', (e) => {
  if ((e.metaKey || e.ctrlKey) && e.code === 'KeyZ') { e.preventDefault(); safe(call(e.shiftKey ? 'redo' : 'undo')); return; }
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

// ---------------------------------------------------------------- polling
let busy = false, lastMidi = 0, meterL = 0, meterR = 0;
const laneLevel = new Map();
function syncInput(id, value) { const i = $(id); if (document.activeElement !== i) i.value = value; }
async function tick() {
  if (busy || !invoke) return;
  busy = true;
  try {
    const s = await call('poll');
    last = s;
    const geometry = s.numerator !== st.numerator || s.denominator !== st.denominator || s.loop !== st.loop || s.loop_bars !== st.loop_bars;
    st = s;
    if (s.version !== lastVersion) await refreshProject();
    else if (geometry) drawAll();

    const pos = s.position_ticks, beat = (PPQ * 4) / s.denominator, bar = beat * s.numerator;
    const time = $('time');
    if (s.playing && pos < 0) { time.textContent = 'Décompte ' + Math.ceil(-pos / beat); time.classList.add('countin'); }
    else { time.classList.remove('countin'); const p = Math.max(0, pos); time.textContent = `${Math.floor(p / bar) + 1} . ${Math.floor((p % bar) / beat) + 1}`; }
    $('btn-rec').classList.toggle('on', s.recording);
    $('btn-play').classList.toggle('on', s.playing && !s.recording);
    syncInput('tempo', s.tempo); syncInput('num', s.numerator); syncInput('den', s.denominator); syncInput('countin', s.count_in); syncInput('loopbars', s.loop_bars);
    $('metro').checked = s.metronome; $('overdub').checked = s.overdub; $('loop').checked = s.loop;
    $('undo').disabled = !s.can_undo; $('redo').disabled = !s.can_redo;

    meterL = Math.max(dbToPct(s.peak_l), meterL * 0.85); meterR = Math.max(dbToPct(s.peak_r), meterR * 0.85);
    $('mL').style.width = meterL * 100 + '%'; $('mR').style.width = meterR * 100 + '%';
    for (const m of s.meters) {
      const v = Math.max(dbToPct(Math.max(m.l, m.r)), (laneLevel.get(m.id) || 0) * 0.85);
      laneLevel.set(m.id, v);
      const e = meterEls.get(m.id); if (e) e.style.width = v * 100 + '%';
    }
    $('load').textContent = Math.round(s.load * 100);
    $('voices').textContent = s.voices;
    const m = s.midi;
    setStatus($('midi-status'), m.inputs.length ? 'Connecté : ' + m.inputs.join(', ') : 'Aucun clavier MIDI détecté (branche-le en USB).');
    $('midi-last').textContent = m.last_message ? '· ' + m.last_message : '';
    $('midi-led').classList.toggle('on', m.message_count !== lastMidi); lastMidi = m.message_count;
    if (s.stream_errors > 0) setStatus($('audio-status'), `${s.stream_errors} erreur(s) du pilote audio — essaie un tampon plus grand.`, true);

    // Playhead across all lanes.
    const body = lanesEl.querySelector('.lane-body');
    if (playheadEl && body && s.playing && pos >= 0) {
      const lr = lanesEl.getBoundingClientRect(), br = body.getBoundingClientRect();
      const x = br.left - lr.left + Math.min(1, pos / spanTicks()) * br.width;
      playheadEl.style.display = 'block'; playheadEl.style.left = x + 'px';
    } else if (playheadEl) playheadEl.style.display = 'none';
  } catch (e) { /* engine not ready yet */ }
  busy = false;
}

// ---------------------------------------------------------------- init
(async function init() {
  applyModeNoRender(store.get('mode', 'beginner'));
  buildPiano(); updateKbdInfo();
  if (!invoke) { setStatus($('audio-status'), 'Lance l\'application avec « cargo tauri dev ».', true); return; }
  try {
    catalog = await call('get_catalog');
    fillMenu($('add-menu'), Object.entries(INSTR), (k) => safe(call('add_track', { kind: k }).then((id) => { selected = id; })));
    fillMenu($('fx-menu'), Object.entries(FX), (k) => { const t = sel(); if (t) safe(call('add_effect', { id: t.id, kind: k })); });
    await loadDevices();
    await refreshProject();
    await applyAudio();
  } catch (e) { setStatus($('audio-status'), String(e), true); }
  setInterval(tick, 33);
})();
function applyModeNoRender(m) {
  document.documentElement.dataset.mode = m;
  $('mode-beginner').classList.toggle('active', m === 'beginner');
  $('mode-pro').classList.toggle('active', m === 'pro');
}
