"use strict";

// ---------------------------------------------------------------- utilitaires
const $ = (s) => document.querySelector(s);
const $$ = (s) => [...document.querySelectorAll(s)];

async function api(path, body, method) {
  const opts = { method: method || (body ? "POST" : "GET"), headers: {} };
  if (body !== undefined) {
    opts.headers["Content-Type"] = "application/json";
    opts.body = JSON.stringify(body);
  }
  const r = await fetch(path, opts);
  const data = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(data.detail || r.statusText);
  return data;
}

function fmt(t, precise = true) {
  if (!isFinite(t)) t = 0;
  const neg = t < 0; t = Math.abs(t);
  const h = Math.floor(t / 3600), m = Math.floor((t % 3600) / 60), s = t % 60;
  const ss = precise ? s.toFixed(1).padStart(4, "0") : String(Math.floor(s)).padStart(2, "0");
  return (neg ? "-" : "") + (h ? `${h}:${String(m).padStart(2, "0")}:` : `${m}:`) + ss;
}

let toastTimer;
function toast(msg, isError = false) {
  const el = $("#toast");
  el.textContent = msg;
  el.classList.toggle("error", isError);
  el.classList.remove("hidden");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.add("hidden"), isError ? 6000 : 3000);
}

// ---------------------------------------------------------------- état
let jobs = [];
let selId = null;
let job = null;          // détail du fichier sélectionné
let peaks = null;        // Uint8Array : -dB par tranche de 10 ms
let peaksFor = null;
let cuts = [];           // régions coupables { start, end, kind, text, on, key }
let keepRegions = [];
const offKeys = new Map(); // jobId -> Set des coupes rétablies à la main
let lastSettings = null;
let listening = null;    // { end } pendant l'écoute d'une coupe
let dirty = true;

const player = $("#player");
const canvas = $("#timeline");
const ctx = canvas.getContext("2d");
const tl = { pps: 50, start: 0, fitted: null };

const STAGE_LABEL = {
  attente: "En file d'attente",
  analyse: "Analyse de l'audio…",
  "aperçu": "Création de l'aperçu vidéo…",
  transcription: "Détection des hésitations (CrisperWhisper)…",
  "prêt": "Prêt",
  erreur: "Erreur",
};

// ---------------------------------------------------------------- fichiers
async function addPaths(paths) {
  if (!paths.length) return;
  try {
    const d = await api("/api/jobs", { paths, settings: lastSettings });
    await poll();
    if (d.jobs.length) select(d.jobs[0].id);
  } catch (e) { toast(e.message, true); }
}

async function pickFiles() {
  try {
    const d = await api("/api/pick", {});
    await addPaths(d.paths);
  } catch (e) { toast(e.message, true); }
}

// Glisser-déposer : dans la fenêtre CrispTrim, Python reçoit les chemins complets et appelle onNativeDrop.
let dragDepth = 0;
const dropOverlay = $("#drop-overlay");
const hasFiles = (e) => [...(e.dataTransfer?.types || [])].includes("Files");
document.addEventListener("dragenter", (e) => {
  if (!hasFiles(e)) return;
  e.preventDefault();
  dragDepth++;
  dropOverlay.classList.remove("hidden");
});
document.addEventListener("dragover", (e) => {
  if (!hasFiles(e)) return;
  e.preventDefault();
  e.dataTransfer.dropEffect = "copy";
});
document.addEventListener("dragleave", () => {
  if (--dragDepth <= 0) { dragDepth = 0; dropOverlay.classList.add("hidden"); }
});
document.addEventListener("drop", (e) => {
  if (!hasFiles(e)) return;
  e.preventDefault();
  dragDepth = 0;
  dropOverlay.classList.add("hidden");
  if (!window.pywebview) {
    toast("Le glisser-déposer marche dans la fenêtre CrispTrim. Ici, utilise « Ajouter des rushes ».", true);
  }
});
window.onNativeDrop = (paths, ignored) => {
  if (ignored) toast(`${ignored} fichier(s) ignoré(s) : ce n'est pas de la vidéo ou de l'audio.`, !paths.length);
  addPaths(paths);
};

$("#btn-add").onclick = pickFiles;
$("#btn-add-2").onclick = pickFiles;
$("#path-form").onsubmit = (e) => {
  e.preventDefault();
  const v = $("#path-input").value.trim();
  if (v) { addPaths([v]); $("#path-input").value = ""; }
};

function renderFiles() {
  const ul = $("#file-list");
  $("#files-empty").classList.toggle("hidden", jobs.length > 0);
  $("#files-count").textContent = jobs.length || "";
  ul.innerHTML = "";
  for (const j of jobs) {
    const li = document.createElement("li");
    li.className = "file" + (j.id === selId ? " active" : "");
    const busy = !["prêt", "erreur"].includes(j.stage);
    const dot = j.stage === "erreur" ? "error" : busy ? "busy" : "ready";
    const dur = j.info ? fmt(j.info.duration, false) : "";
    const gain = j.stats && j.stats.removed_pct !== undefined && !busy ? `−${j.stats.removed_pct} %` : "";
    const thumb = j.thumb ? `style="background-image:url('/api/jobs/${j.id}/thumb')"` : "";
    const icon = j.thumb ? "" : j.kind === "audio" ? "♪" : "▶";
    li.innerHTML = `
      <div class="thumb" ${thumb}>${icon}</div>
      <div class="info">
        <div class="name"></div>
        <div class="meta"><i class="dot ${dot}"></i><span class="stage"></span><span class="gain">${gain}</span></div>
      </div>
      ${busy ? `<div class="progress"><i></i></div>` : ""}
      <button class="remove" title="Retirer">✕</button>`;
    li.querySelector(".name").textContent = j.name;
    li.querySelector(".name").title = j.path;
    li.querySelector(".stage").textContent = busy || j.stage === "erreur" ? STAGE_LABEL[j.stage] : dur;
    li.onclick = () => select(j.id);
    li.querySelector(".remove").onclick = async (e) => {
      e.stopPropagation();
      await api(`/api/jobs/${j.id}`, undefined, "DELETE");
      if (selId === j.id) { selId = null; job = null; player.removeAttribute("src"); player.load(); }
      await poll();
    };
    ul.appendChild(li);
  }
  const has = selId && jobs.some((j) => j.id === selId);
  $("#empty").classList.toggle("hidden", !!has);
  $("#editor").classList.toggle("hidden", !has);
}

async function select(id) {
  if (selId === id) return;
  selId = id;
  job = null;
  tl.fitted = null;
  renderFiles();
  await loadDetail(id);
}

async function loadDetail(id) {
  const needPeaks = peaksFor !== id;
  const d = await api(`/api/jobs/${id}?peaks=${needPeaks ? 1 : 0}`);
  if (id !== selId) return;
  if (needPeaks && d.peaks) {
    peaks = Uint8Array.from(atob(d.peaks), (c) => c.charCodeAt(0));
    peaksFor = id;
  }
  applyDetail(d);
}

function applyDetail(d) {
  const switched = !job || job.id !== d.id;
  job = d;
  if (!offKeys.has(d.id)) offKeys.set(d.id, new Set());
  const off = offKeys.get(d.id);
  keepRegions = (d.regions || []).filter((r) => r.kind === "keep");
  cuts = (d.regions || []).filter((r) => r.kind !== "keep").map((r) => {
    const key = `${r.start.toFixed(2)}-${r.end.toFixed(2)}`;
    return { ...r, key, on: !off.has(key) };
  });

  if (switched) fillSettings(d.settings);
  if (d.media_ready && player.dataset.job !== d.id) {
    player.dataset.job = d.id;
    player.src = `/api/jobs/${d.id}/media`;
  }
  if (d.info && tl.fitted !== d.id) fitTimeline();

  // superpositions d'état
  const overlay = $("#player-overlay");
  if (d.stage === "erreur") {
    overlay.textContent = "Erreur : " + d.error;
    overlay.classList.remove("hidden");
  } else if (!d.media_ready) {
    overlay.innerHTML = `<div><div class="spinner"></div><div></div></div>`;
    overlay.querySelector("div div:last-child").textContent = STAGE_LABEL[d.stage] || "…";
    overlay.classList.remove("hidden");
  } else if (d.info && !d.info.has_video) {
    overlay.innerHTML = `<div><div class="audio-icon">♪</div><div>Fichier audio</div></div>`;
    overlay.classList.remove("hidden");
  } else overlay.classList.add("hidden");

  const st = $("#tl-status");
  const busy = !["prêt", "erreur"].includes(d.stage);
  st.textContent = STAGE_LABEL[d.stage] || "";
  st.classList.toggle("hidden", !busy);

  renderCuts();
  renderTranscript();
  renderStats();
  renderExport();
  dirty = true;
}

// ---------------------------------------------------------------- sondage
async function poll() {
  try {
    const d = await api("/api/jobs");
    jobs = d.jobs;
    if (selId && !jobs.some((j) => j.id === selId)) selId = null;
    renderFiles();
    if (!selId && jobs.length) { await select(jobs[0].id); return; }
    const j = jobs.find((x) => x.id === selId);
    if (j && (!job || job.id !== j.id || job.version !== j.version)) await loadDetail(j.id);
  } catch (e) { /* serveur en cours de redémarrage */ }
}
setInterval(poll, 800);

// ---------------------------------------------------------------- réglages
const S = {
  silences: $("#s-silences"), auto: $("#s-auto"), threshold: $("#s-threshold"),
  minsil: $("#s-minsil"), margin: $("#s-margin"), fillers: $("#s-fillers"), noises: $("#s-noises"),
};

function fillSettings(s) {
  S.silences.checked = s.cut_silences;
  S.auto.checked = s.threshold_db === null;
  S.threshold.value = s.threshold_db ?? Math.round(job?.threshold ?? -45);
  S.minsil.value = s.min_silence;
  S.margin.value = s.margin;
  S.fillers.checked = s.cut_fillers;
  S.noises.checked = s.cut_noises;
  lastSettings = s;
  updateOutputs();
}

function readSettings() {
  return {
    cut_silences: S.silences.checked,
    threshold_db: S.auto.checked ? null : Number(S.threshold.value),
    min_silence: Number(S.minsil.value),
    margin: Number(S.margin.value),
    min_keep: lastSettings?.min_keep ?? 0.25,
    cut_fillers: S.fillers.checked,
    cut_noises: S.noises.checked,
  };
}

function updateOutputs() {
  S.threshold.disabled = S.auto.checked;
  const thr = S.auto.checked ? (job ? job.threshold : null) : Number(S.threshold.value);
  $("#o-threshold").textContent = thr === null ? "auto" : `${Math.round(thr)} dB`;
  if (S.auto.checked && job) S.threshold.value = Math.round(job.threshold);
  $("#o-minsil").textContent = `${Number(S.minsil.value).toFixed(2)} s`;
  $("#o-margin").textContent = `${Math.round(Number(S.margin.value) * 1000)} ms`;
  for (const r of [S.threshold, S.minsil, S.margin]) {
    r.style.setProperty("--fill", `${(100 * (r.value - r.min)) / (r.max - r.min)}%`);
  }
}

let settingsTimer;
function onSettings() {
  updateOutputs();
  lastSettings = readSettings();
  dirty = true;
  clearTimeout(settingsTimer);
  settingsTimer = setTimeout(async () => {
    if (!job) return;
    try { applyDetail(await api(`/api/jobs/${job.id}/settings`, lastSettings)); }
    catch (e) { toast(e.message, true); }
  }, 180);
}
Object.values(S).forEach((el) => el.addEventListener("input", onSettings));

// ---------------------------------------------------------------- modèle
api("/api/config").then((c) => { $("#model-select").value = c.model; });
$("#model-select").onchange = async (e) => {
  await api("/api/config", { model: e.target.value });
  toast(`Modèle « ${e.target.value} » : utilisé pour les prochains fichiers.`);
};

// ---------------------------------------------------------------- coupes
function setCut(c, on) {
  c.on = on;
  const off = offKeys.get(job.id);
  on ? off.delete(c.key) : off.add(c.key);
  renderCuts(); renderTranscript(); renderStats();
  dirty = true;
}

function renderCuts() {
  const filter = $("#cuts-filter").value;
  const list = $("#cuts-list");
  list.innerHTML = "";
  $("#cuts-count").textContent = cuts.filter((c) => c.on).length;
  const frag = document.createDocumentFragment();
  for (const c of cuts) {
    if (filter !== "all" && c.kind !== filter) continue;
    const li = document.createElement("li");
    li.className = "cut" + (c.on ? "" : " off");
    const label = c.kind === "filler" ? "Hésitation" : "Silence";
    const color = c.kind === "filler" ? "var(--filler)" : "#7a8191";
    li.innerHTML = `
      <input type="checkbox" ${c.on ? "checked" : ""} title="Couper">
      <div><div class="kind"><i class="sw" style="background:${color}"></i>${label}</div>
      <div class="when">${fmt(c.start)} → ${fmt(c.end)} · ${(c.end - c.start).toFixed(2)} s</div></div>
      <button class="icon-btn listen" title="Écouter">▶</button>
      ${c.text ? `<div class="text"></div>` : ""}`;
    if (c.text) li.querySelector(".text").textContent = c.text;
    li.querySelector("input").onclick = (e) => { e.stopPropagation(); setCut(c, e.target.checked); };
    li.querySelector(".listen").onclick = (e) => { e.stopPropagation(); listen(c); };
    li.onclick = () => { seek(Math.max(0, c.start - 0.5)); centerOn(c.start); };
    frag.appendChild(li);
  }
  list.appendChild(frag);
}
$("#cuts-filter").onchange = renderCuts;
$("#btn-all-on").onclick = () => { cuts.forEach((c) => (c.on = true)); offKeys.get(job.id).clear(); renderCuts(); renderStats(); renderTranscript(); dirty = true; };
$("#btn-all-off").onclick = () => { cuts.forEach((c) => { c.on = false; offKeys.get(job.id).add(c.key); }); renderCuts(); renderStats(); renderTranscript(); dirty = true; };

function listen(c) {
  listening = { end: Math.min(job.info.duration, c.end + 0.7) };
  player.currentTime = Math.max(0, c.start - 0.7);
  player.play();
}

function finalKeep() {
  const parts = [...keepRegions, ...cuts.filter((c) => !c.on)]
    .map((r) => [r.start, r.end]).sort((a, b) => a[0] - b[0]);
  const out = [];
  for (const [s, e] of parts) {
    if (out.length && s <= out[out.length - 1][1] + 1e-6) out[out.length - 1][1] = Math.max(out[out.length - 1][1], e);
    else out.push([s, e]);
  }
  return out;
}

function activeCutAt(t) {
  for (const c of cuts) {
    if (c.start > t) break;
    if (c.on && t >= c.start && t < c.end - 0.02) return c;
  }
  return null;
}

function editedTime(t) {
  let acc = 0;
  for (const [s, e] of finalKeep()) {
    if (t <= s) break;
    acc += Math.min(t, e) - s;
  }
  return acc;
}

function renderStats() {
  if (!job || !job.info) return;
  const orig = job.info.duration;
  const fin = finalKeep().reduce((a, [s, e]) => a + e - s, 0);
  $("#st-orig").textContent = fmt(orig, false);
  $("#st-final").textContent = fmt(fin, false);
  $("#st-gain").textContent = orig ? `−${Math.round((100 * (orig - fin)) / orig)} %` : "–";
  $("#st-bar").style.width = orig ? `${(100 * fin) / orig}%` : "0";
}

// ---------------------------------------------------------------- transcription
let wordEls = [];
let curWord = -1;
function renderTranscript() {
  const box = $("#transcript");
  box.innerHTML = "";
  wordEls = [];
  curWord = -1;
  if (!job) return;
  if (!job.words.length) {
    box.innerHTML = `<p class="muted">${job.settings.cut_fillers
      ? (job.stage === "transcription" ? "Transcription en cours…" : "Pas de transcription.")
      : "Active « Couper les hésitations » pour transcrire."}</p>`;
    return;
  }
  const frag = document.createDocumentFragment();
  let prevEnd = 0;
  for (const w of job.words) {
    if (w.start - prevEnd > 1) {
      const g = document.createElement("span");
      g.className = "gap";
      g.textContent = `⏸ ${(w.start - prevEnd).toFixed(1)} s`;
      frag.appendChild(g);
    }
    const sp = document.createElement("span");
    const mid = (w.start + w.end) / 2;
    const cut = cuts.find((c) => c.kind === "filler" && mid >= c.start && mid <= c.end);
    sp.className = "w" + (cut ? " filler" + (cut.on ? " struck" : "") : "");
    sp.textContent = w.word;
    sp.onclick = () => { seek(w.start); centerOn(w.start); };
    frag.appendChild(sp);
    frag.appendChild(document.createTextNode(" "));
    wordEls.push(sp);
    prevEnd = w.end;
  }
  box.appendChild(frag);
}

function highlightWord(t) {
  if (!job || !wordEls.length) return;
  const ws = job.words;
  let lo = 0, hi = ws.length - 1, idx = -1;
  while (lo <= hi) {
    const m = (lo + hi) >> 1;
    if (ws[m].start <= t) { idx = m; lo = m + 1; } else hi = m - 1;
  }
  if (idx >= 0 && t > ws[idx].end + 0.3) idx = -1;
  if (idx === curWord) return;
  if (curWord >= 0) wordEls[curWord]?.classList.remove("now");
  curWord = idx;
  if (idx >= 0) wordEls[idx].classList.add("now");
}

// ---------------------------------------------------------------- export
$$("[data-export]").forEach((b) => {
  b.onclick = async () => {
    if (!job) return;
    try {
      await api(`/api/jobs/${job.id}/export`, { formats: [b.dataset.export], keep: finalKeep() });
      await poll();
      const msg = {
        fcpxml: "Timeline DaVinci / Final Cut exportée",
        premiere: "Timeline Premiere exportée",
        vegas: "EDL Vegas exportée (Fichier > Importer > Fichier texte EDL)",
        srt: "Sous-titres exportés",
        txt: "Texte exporté",
      };
      if (msg[b.dataset.export]) toast(`${msg[b.dataset.export]}, à côté du rush.`);
    } catch (e) { toast(e.message, true); }
  };
});

function renderExport() {
  const el = $("#export-status");
  const ready = job && job.stage === "prêt";
  $$("[data-export]").forEach((b) => {
    const needsWords = ["srt", "txt"].includes(b.dataset.export);
    b.disabled = !ready || job.export.state === "rendu" || (needsWords && !job.words.length);
    b.title = needsWords && job && !job.words.length ? "Active « Couper les hésitations » pour transcrire" : "";
  });
  if (!job) { el.innerHTML = ""; return; }
  const ex = job.export;
  if (ex.state === "rendu") el.textContent = "Rendu de la vidéo montée…";
  else if (ex.state === "erreur") el.textContent = "Erreur : " + ex.error;
  else if (ex.files.length) {
    el.innerHTML = "";
    for (const f of [...ex.files].reverse()) {
      const div = document.createElement("div");
      div.innerHTML = `<span class="ok">✓</span><span class="fname"></span><a>Afficher</a>`;
      div.querySelector(".fname").textContent = f.split(/[\\/]/).pop();
      div.querySelector(".fname").title = f;
      div.querySelector("a").onclick = () => api("/api/reveal", { path: f });
      el.appendChild(div);
    }
  } else el.textContent = ready ? "Les fichiers sont créés à côté du rush." : "";
}

// ---------------------------------------------------------------- lecteur
function seek(t) {
  listening = null;
  player.currentTime = Math.max(0, Math.min(t, job?.info?.duration ?? t));
  dirty = true;
}

$("#btn-play").onclick = () => (player.paused ? player.play() : player.pause());
player.onplay = () => ($("#btn-play").textContent = "❚❚");
player.onpause = () => ($("#btn-play").textContent = "▶");
$("#preview-toggle").onchange = () => (dirty = true);

function jumpCut(dir) {
  if (!cuts.length) return;
  const t = player.currentTime;
  const c = dir > 0 ? cuts.find((c) => c.start > t + 0.6) : [...cuts].reverse().find((c) => c.start < t - 0.6);
  if (c) { seek(Math.max(0, c.start - 0.5)); centerOn(c.start); }
}
$("#btn-prev").onclick = () => jumpCut(-1);
$("#btn-next").onclick = () => jumpCut(1);

document.addEventListener("keydown", (e) => {
  if (e.target.matches("input[type=text], select") || !job) return;
  if (document.querySelector(".modal-backdrop:not(.hidden)")) return;
  if (e.code === "Space") { e.preventDefault(); $("#btn-play").click(); }
  else if (e.key === "p" || e.key === "P") { $("#preview-toggle").click(); }
  else if (e.key === "ArrowRight") { e.preventDefault(); jumpCut(1); }
  else if (e.key === "ArrowLeft") { e.preventDefault(); jumpCut(-1); }
});

// ---------------------------------------------------------------- onglets
$$(".tab").forEach((t) => {
  t.onclick = () => {
    $$(".tab").forEach((x) => x.classList.toggle("active", x === t));
    $$(".tab-body").forEach((b) => b.classList.toggle("hidden", b.dataset.body !== t.dataset.tab));
  };
});

// ---------------------------------------------------------------- timeline
const RULER = 20;

function viewDur() { return canvas.clientWidth / tl.pps; }
function fitTimeline() {
  if (!job?.info) return;
  tl.pps = Math.max(1e-3, (canvas.clientWidth - 2) / job.info.duration);
  tl.start = 0;
  tl.fitted = job.id;
  dirty = true;
}
function clampView() {
  const dur = job?.info?.duration ?? 0;
  const minPps = (canvas.clientWidth - 2) / Math.max(dur, 0.1);
  tl.pps = Math.min(Math.max(tl.pps, minPps), 600);
  tl.start = Math.min(Math.max(0, tl.start), Math.max(0, dur - viewDur()));
}
function zoom(factor, anchorX = canvas.clientWidth / 2) {
  const t = tl.start + anchorX / tl.pps;
  tl.pps *= factor;
  clampView();
  tl.start = t - anchorX / tl.pps;
  clampView();
  dirty = true;
}
function centerOn(t) {
  const vd = viewDur();
  if (t < tl.start || t > tl.start + vd) { tl.start = t - vd * 0.3; clampView(); }
  dirty = true;
}
$("#btn-zoom-in").onclick = () => zoom(1.6);
$("#btn-zoom-out").onclick = () => zoom(1 / 1.6);
$("#btn-zoom-fit").onclick = fitTimeline;

function xToT(x) { return tl.start + x / tl.pps; }

let dragging = false;
canvas.addEventListener("mousedown", (e) => {
  if (!job?.info) return;
  dragging = true;
  seek(xToT(e.offsetX));
});
window.addEventListener("mousemove", (e) => {
  if (!dragging) return;
  const r = canvas.getBoundingClientRect();
  seek(xToT(Math.min(Math.max(0, e.clientX - r.left), r.width)));
});
window.addEventListener("mouseup", () => (dragging = false));
canvas.addEventListener("dblclick", (e) => {
  const t = xToT(e.offsetX);
  const c = cuts.find((c) => t >= c.start && t <= c.end);
  if (c) setCut(c, !c.on);
});
canvas.addEventListener("wheel", (e) => {
  if (!job?.info) return;
  e.preventDefault();
  if (e.ctrlKey || e.metaKey) zoom(e.deltaY < 0 ? 1.25 : 0.8, e.offsetX);
  else { tl.start += (e.deltaY + e.deltaX) / tl.pps; clampView(); dirty = true; }
}, { passive: false });
new ResizeObserver(() => { if (job && tl.fitted === job.id) { clampView(); } dirty = true; }).observe(canvas);

let hatch = null;
function hatchPattern() {
  if (hatch) return hatch;
  const c = document.createElement("canvas");
  c.width = c.height = 8;
  const g = c.getContext("2d");
  g.strokeStyle = "rgba(140,148,165,.22)";
  g.lineWidth = 2;
  g.beginPath(); g.moveTo(0, 8); g.lineTo(8, 0); g.moveTo(-2, 2); g.lineTo(2, -2); g.moveTo(6, 10); g.lineTo(10, 6); g.stroke();
  return (hatch = ctx.createPattern(c, "repeat"));
}

function draw() {
  const dpr = window.devicePixelRatio || 1;
  const W = canvas.clientWidth, H = canvas.clientHeight;
  if (canvas.width !== Math.round(W * dpr) || canvas.height !== Math.round(H * dpr)) {
    canvas.width = Math.round(W * dpr); canvas.height = Math.round(H * dpr);
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = "#11141b";
  ctx.fillRect(0, 0, W, H);
  if (!job?.info) return;

  const dur = job.info.duration;
  const t0 = tl.start, t1 = tl.start + W / tl.pps;
  const X = (t) => (t - t0) * tl.pps;
  const bodyTop = RULER, bodyH = H - RULER;

  // régions
  ctx.fillStyle = "#1f2636";
  ctx.fillRect(Math.max(0, X(0)), bodyTop, Math.min(W, X(dur)) - Math.max(0, X(0)), bodyH);
  for (const c of cuts) {
    if (c.end < t0 || c.start > t1) continue;
    const x0 = X(c.start), w = Math.max(1, X(c.end) - x0);
    if (!c.on) {
      ctx.strokeStyle = c.kind === "filler" ? "rgba(255,157,66,.6)" : "rgba(160,168,185,.45)";
      ctx.setLineDash([4, 3]);
      ctx.strokeRect(x0 + 0.5, bodyTop + 0.5, w - 1, bodyH - 1);
      ctx.setLineDash([]);
      continue;
    }
    if (c.kind === "filler") {
      ctx.fillStyle = "rgba(255,157,66,.22)";
      ctx.fillRect(x0, bodyTop, w, bodyH);
      ctx.fillStyle = "#ff9d42";
      ctx.fillRect(x0, bodyTop, w, 3);
    } else {
      ctx.fillStyle = "#11141b";
      ctx.fillRect(x0, bodyTop, w, bodyH);
      ctx.fillStyle = hatchPattern();
      ctx.fillRect(x0, bodyTop, w, bodyH);
    }
  }

  // forme d'onde
  if (peaks && peaksFor === job.id) {
    const frame = job.frame || 0.01;
    const mid = bodyTop + bodyH / 2, amp = bodyH / 2 - 6;
    const FLOOR = 70;
    let ci = 0;
    for (let x = 0; x < W; x++) {
      const ta = t0 + x / tl.pps, tb = t0 + (x + 1) / tl.pps;
      if (ta > dur) break;
      let i0 = Math.floor(ta / frame), i1 = Math.max(i0 + 1, Math.floor(tb / frame));
      i1 = Math.min(i1, peaks.length);
      let best = 255;
      for (let i = i0; i < i1; i++) if (peaks[i] < best) best = peaks[i];
      const a = Math.max(0, Math.min(1, (FLOOR - best) / FLOOR));
      const h = Math.max(1, a * amp);
      while (ci < cuts.length && cuts[ci].end < ta) ci++;
      const inCut = ci < cuts.length && cuts[ci].on && ta >= cuts[ci].start && ta <= cuts[ci].end;
      ctx.fillStyle = inCut ? "#4b5160" : "#a6b2d4";
      ctx.fillRect(x, mid - h, 1, h * 2);
    }
    // seuil de silence
    const thr = S.auto.checked ? job.threshold : Number(S.threshold.value);
    const at = Math.max(0, Math.min(1, (FLOOR + thr) / FLOOR)) * amp;
    ctx.strokeStyle = "rgba(143,123,255,.55)";
    ctx.setLineDash([3, 4]);
    ctx.beginPath();
    ctx.moveTo(0, mid - at); ctx.lineTo(W, mid - at);
    ctx.moveTo(0, mid + at); ctx.lineTo(W, mid + at);
    ctx.stroke();
    ctx.setLineDash([]);
  }

  // règle
  ctx.fillStyle = "#0e1117";
  ctx.fillRect(0, 0, W, RULER);
  const steps = [0.1, 0.2, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 1800];
  const step = steps.find((s) => s * tl.pps >= 80) || 3600;
  ctx.fillStyle = "#6f7787";
  ctx.strokeStyle = "#2a2f3a";
  ctx.font = "11px Segoe UI, system-ui, sans-serif";
  ctx.beginPath();
  for (let t = Math.floor(t0 / step) * step; t <= t1; t += step) {
    const x = Math.round(X(t)) + 0.5;
    ctx.moveTo(x, RULER - 6); ctx.lineTo(x, RULER);
    ctx.fillText(fmt(t, step < 1), x + 4, 13);
  }
  ctx.stroke();

  // tête de lecture
  const px = X(player.currentTime);
  if (px >= -2 && px <= W + 2) {
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(Math.round(px) - 1, 0, 2, H);
    ctx.beginPath();
    ctx.moveTo(px - 6, 0); ctx.lineTo(px + 6, 0); ctx.lineTo(px, 8); ctx.fill();
  }
}

// ---------------------------------------------------------------- boucle
function tick() {
  if (job?.info) {
    const t = player.currentTime;
    if (!player.paused) {
      if (listening) {
        if (t >= listening.end) { player.pause(); listening = null; }
      } else if ($("#preview-toggle").checked) {
        const c = activeCutAt(t);
        if (c) player.currentTime = c.end;
      }
      // la timeline suit la lecture
      const vd = viewDur();
      if (!dragging && (t > tl.start + vd * 0.92 || t < tl.start)) { tl.start = t - vd * 0.1; clampView(); }
      dirty = true;
    }
    if (dirty) {
      draw();
      $("#time-orig").textContent = fmt(t);
      $("#time-edit").textContent = fmt(editedTime(t));
      highlightWord(t);
      dirty = false;
    }
  } else if (dirty) { draw(); dirty = false; }
  requestAnimationFrame(tick);
}
player.addEventListener("seeked", () => (dirty = true));
player.addEventListener("loadedmetadata", () => (dirty = true));
requestAnimationFrame(tick);
poll();

// ---------------------------------------------------------------- version, crédits, mises à jour
const SKIP_KEY = "crisptrim.skipVersion";
let update = null;

function lsGet(k) { try { return localStorage.getItem(k); } catch { return null; } }
function lsSet(k, v) { try { localStorage.setItem(k, v); } catch { /* stockage indisponible */ } }

function openModal(sel) { $(sel).classList.remove("hidden"); }
function closeModals() { $$(".modal-backdrop").forEach((m) => m.classList.add("hidden")); }
$$(".modal-backdrop").forEach((m) => {
  m.addEventListener("click", (e) => { if (e.target === m || e.target.closest("[data-close]")) closeModals(); });
});
document.addEventListener("keydown", (e) => { if (e.key === "Escape") closeModals(); });

// Les liens s'ouvrent dans le navigateur du système, pas dans la fenêtre de l'appli.
document.addEventListener("click", (e) => {
  const a = e.target.closest("[data-href]");
  if (!a) return;
  e.preventDefault();
  api("/api/open-url", { url: a.dataset.href }).catch((err) => toast(err.message, true));
});

function renderUpdate() {
  const st = $("#about-update");
  st.className = "update-state";
  st.onclick = null;
  if (!update) st.textContent = "vérification…";
  else if (update.available) {
    st.innerHTML = `<a></a>`;
    st.firstChild.textContent = `${update.latest} disponible`;
    st.classList.add("new");
    st.onclick = () => { closeModals(); openModal("#update-modal"); };
  } else if (update.error && !update.latest) st.textContent = "impossible de vérifier (hors ligne ?)";
  else { st.textContent = "à jour"; st.classList.add("ok"); }

  $("#update-pill").classList.toggle("hidden", !(update?.available && lsGet(SKIP_KEY) !== update.latest));
  if (update?.available) {
    $("#update-pill-tag").textContent = update.latest;
    $("#update-tag").textContent = update.release.name || update.latest;
    $("#update-current").textContent = `v${update.current}`;
    $("#update-notes").textContent = update.release.notes || "";
  }
}

async function checkUpdate(force = false) {
  try { update = await api(`/api/update${force ? "?force=1" : ""}`); }
  catch (e) { update = { available: false, latest: null, error: e.message }; }
  renderUpdate();
}

api("/api/version").then((v) => { $("#about-version").textContent = v.version; }).catch(() => {});
$("#brand").onclick = $("#btn-about").onclick = () => { renderUpdate(); openModal("#about-modal"); };
$("#update-pill").onclick = () => openModal("#update-modal");
$("#update-open").onclick = () => api("/api/open-url", { url: update.release.url }).catch((e) => toast(e.message, true));
$("#update-later").onclick = () => { lsSet(SKIP_KEY, update.latest); closeModals(); renderUpdate(); };
$("#about-check").onclick = async () => {
  update = null;
  renderUpdate();
  await checkUpdate(true);
  if (update.available) { closeModals(); openModal("#update-modal"); }
  else toast(update.error && !update.latest ? "Impossible de joindre GitHub." : "CrispTrim est à jour.", !!(update.error && !update.latest));
};
setTimeout(checkUpdate, 1500);
