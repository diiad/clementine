// Trim (audio, video) and crop (images) editor. The page picks a range or a
// rectangle and hands the core a parameterised operation:
// `tool:trim=<start>-<end>` in seconds, `tool:crop=<x>,<y>,<w>,<h>` in pixels.
const { invoke, convertFileSrc } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);

const FR = {
  "No preview for this format: enter the values below.":
    "Pas d’aperçu pour ce format : saisissez les valeurs ci-dessous.",
  "Click to move the playhead": "Cliquez pour déplacer la tête de lecture",
  "Start": "Début",
  "End": "Fin",
  "Start here": "Début ici",
  "End here": "Fin ici",
  "Ratio": "Format",
  "Free": "Libre",
  "Cancel": "Annuler",
  "Trim": "Découper",
  "Crop": "Recadrer",
  "Working…": "En cours…",
  "Kept": "Durée gardée",
};
let lang = "en";
const t = (en) => (lang === "fr" && FR[en]) || en;

function translatePage() {
  document.documentElement.lang = lang;
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    const key = n.nodeValue.trim();
    if (key && FR[key]) n.nodeValue = n.nodeValue.replace(key, t(key));
  }
  for (const el of document.querySelectorAll("[title]")) el.title = t(el.title);
  for (const o of $("ratio").options) o.textContent = t(o.textContent);
}

let file = null;
let busy = false;

// --- Trim ------------------------------------------------------------------------

let duration = 0;
let start = 0;
let end = 0;
let media = null; // the <video> or <audio> element, when it can play the file

function fmt(s) {
  const m = Math.floor(s / 60);
  const sec = (s - m * 60).toFixed(1).padStart(4, "0");
  return m >= 60 ? `${Math.floor(m / 60)}:${String(m % 60).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}

function parseTime(text) {
  const parts = text.trim().split(":").map(Number);
  if (!parts.length || parts.some((p) => Number.isNaN(p))) return null;
  return parts.reduce((acc, p) => acc * 60 + p, 0);
}

function renderTrim() {
  const pct = (s) => (duration ? (s / duration) * 100 : 0);
  $("range").style.left = `${pct(start)}%`;
  $("range").style.width = `${pct(end) - pct(start)}%`;
  $("h-start").style.left = `${pct(start)}%`;
  $("h-end").style.left = `${pct(end)}%`;
  $("playhead").style.left = `${pct(media ? media.currentTime : start)}%`;
  if (document.activeElement !== $("start")) $("start").value = fmt(start);
  if (document.activeElement !== $("end")) $("end").value = fmt(end);
  $("length").textContent = `${t("Kept")} ${fmt(Math.max(0, end - start))}`;
  $("apply").disabled = busy || end - start < 0.05;
}

const clampTime = (s) => Math.min(Math.max(0, s), duration);
const setStart = (s) => { start = Math.min(clampTime(s), end - 0.05); renderTrim(); };
const setEnd = (s) => { end = Math.max(clampTime(s), start + 0.05); renderTrim(); };

function trackTime(e) {
  const r = $("track").getBoundingClientRect();
  return clampTime(((e.clientX - r.left) / r.width) * duration);
}

function seek(s) {
  if (media) media.currentTime = s;
  renderTrim();
}

function setupTrim() {
  duration = file.duration || 0;
  start = 0;
  end = duration;
  $("trim-tools").hidden = false;
  $("apply").textContent = t("Trim");
  const el = file.kind === "video" ? $("video") : $("audio");
  el.hidden = false;
  el.src = convertFileSrc(file.path);
  media = el;
  el.addEventListener("loadedmetadata", () => {
    if (!duration && Number.isFinite(el.duration)) {
      duration = el.duration;
      end = duration;
    }
    renderTrim();
  });
  // WebKit can't play MKV, AVI, WMV…: keep the timeline, drop the player.
  el.addEventListener("error", () => {
    el.hidden = true;
    media = null;
    $("no-preview").hidden = false;
    renderTrim();
  });
  el.addEventListener("timeupdate", () => {
    // Play only the kept range.
    if (!el.paused && el.currentTime >= end) {
      el.pause();
      el.currentTime = end;
    }
    renderTrim();
  });
  el.addEventListener("play", () => {
    if (el.currentTime >= end - 0.05 || el.currentTime < start) el.currentTime = start;
  });

  let dragging = null;
  $("track").addEventListener("pointerdown", (e) => {
    dragging = e.target.id === "h-start" ? "start" : e.target.id === "h-end" ? "end" : "seek";
    $("track").setPointerCapture(e.pointerId);
    if (dragging === "seek") seek(trackTime(e));
  });
  $("track").addEventListener("pointermove", (e) => {
    if (!dragging) return;
    const s = trackTime(e);
    if (dragging === "start") setStart(s);
    else if (dragging === "end") setEnd(s);
    else seek(s);
    if (dragging !== "seek") seek(s);
  });
  $("track").addEventListener("pointerup", () => (dragging = null));

  const current = () => (media ? media.currentTime : start);
  $("mark-start").addEventListener("click", () => setStart(current()));
  $("mark-end").addEventListener("click", () => setEnd(current()));
  for (const [id, set] of [["start", setStart], ["end", setEnd]]) {
    $(id).addEventListener("change", (e) => {
      const s = parseTime(e.target.value);
      if (s !== null) set(s);
      e.target.blur();
      renderTrim();
    });
  }
  renderTrim();
}

// --- Crop --------------------------------------------------------------------------

let natural = { w: 0, h: 0 };
let rect = { x: 0, y: 0, w: 0, h: 0 }; // in image pixels

const ratio = () => Number($("ratio").value);

function renderCrop() {
  const img = $("image");
  const k = img.clientWidth / (natural.w || 1);
  Object.assign($("crop").style, {
    left: `${rect.x * k}px`,
    top: `${rect.y * k}px`,
    width: `${rect.w * k}px`,
    height: `${rect.h * k}px`,
  });
  for (const [id, key] of [["cx", "x"], ["cy", "y"], ["cw", "w"], ["ch", "h"]]) {
    if (document.activeElement !== $(id)) $(id).value = Math.round(rect[key]);
  }
  $("apply").disabled = busy || rect.w < 1 || rect.h < 1;
}

/// Keep the rectangle inside the image (when its size is known).
function clampRect(r) {
  if (!natural.w) return r;
  const w = Math.min(Math.max(1, r.w), natural.w);
  const h = Math.min(Math.max(1, r.h), natural.h);
  return {
    w,
    h,
    x: Math.min(Math.max(0, r.x), natural.w - w),
    y: Math.min(Math.max(0, r.y), natural.h - h),
  };
}

/// The largest rectangle of the chosen ratio, centred.
function applyRatio() {
  const q = ratio();
  if (!q || !natural.w) return;
  let w = natural.w;
  let h = w / q;
  if (h > natural.h) {
    h = natural.h;
    w = h * q;
  }
  rect = { x: (natural.w - w) / 2, y: (natural.h - h) / 2, w, h };
  renderCrop();
}

function setupCrop() {
  $("crop-tools").hidden = false;
  $("apply").textContent = t("Crop");
  const img = $("image");
  $("picture").hidden = false;
  img.addEventListener("load", () => {
    natural = { w: img.naturalWidth, h: img.naturalHeight };
    rect = { x: 0, y: 0, w: natural.w, h: natural.h };
    renderCrop();
  });
  img.addEventListener("error", () => {
    $("picture").hidden = true;
    $("no-preview").hidden = false;
    $("ratio").disabled = true;
    rect = { x: 0, y: 0, w: 0, h: 0 };
    renderCrop();
  });
  img.src = convertFileSrc(file.path);
  window.addEventListener("resize", renderCrop);
  $("ratio").addEventListener("change", applyRatio);

  let drag = null;
  $("crop").addEventListener("pointerdown", (e) => {
    drag = { handle: e.target.dataset.h || "move", x: e.clientX, y: e.clientY, from: { ...rect } };
    $("crop").setPointerCapture(e.pointerId);
    e.preventDefault();
  });
  $("crop").addEventListener("pointermove", (e) => {
    if (!drag) return;
    const k = natural.w / img.clientWidth;
    const dx = (e.clientX - drag.x) * k;
    const dy = (e.clientY - drag.y) * k;
    const f = drag.from;
    if (drag.handle === "move") {
      rect = clampRect({ ...f, x: f.x + dx, y: f.y + dy });
      return renderCrop();
    }
    // Resize from a corner; the opposite corner stays put.
    const west = drag.handle.includes("w");
    const north = drag.handle.includes("n");
    let w = Math.max(8, west ? f.w - dx : f.w + dx);
    let h = Math.max(8, north ? f.h - dy : f.h + dy);
    const q = ratio();
    if (q) {
      if (w / h > q) w = h * q;
      else h = w / q;
    }
    const right = f.x + f.w;
    const bottom = f.y + f.h;
    let x = west ? right - w : f.x;
    let y = north ? bottom - h : f.y;
    // Stay inside the picture without moving the anchored corner.
    if (x < 0) { w += x; x = 0; if (q) h = w / q; if (north) y = bottom - h; }
    if (y < 0) { h += y; y = 0; if (q) w = h * q; if (west) x = right - w; }
    if (x + w > natural.w) { w = natural.w - x; if (q) { h = w / q; if (north) y = bottom - h; } }
    if (y + h > natural.h) { h = natural.h - y; if (q) { w = h * q; if (west) x = right - w; } }
    rect = { x, y, w, h };
    renderCrop();
  });
  $("crop").addEventListener("pointerup", () => (drag = null));

  for (const [id, key] of [["cx", "x"], ["cy", "y"], ["cw", "w"], ["ch", "h"]]) {
    $(id).addEventListener("change", (e) => {
      const v = Number(e.target.value);
      if (Number.isFinite(v)) rect = clampRect({ ...rect, [key]: v });
      renderCrop();
    });
  }
  renderCrop();
}

// --- Apply -------------------------------------------------------------------------

function operation() {
  if (file.kind === "image") {
    const [x, y, w, h] = [rect.x, rect.y, rect.w, rect.h].map(Math.round);
    return `tool:crop=${x},${y},${w},${h}`;
  }
  return `tool:trim=${start.toFixed(3)}-${end.toFixed(3)}`;
}

function setBusy(on, text = "") {
  busy = on;
  $("status").textContent = text;
  $("status").classList.remove("error");
  for (const id of ["cancel", "apply"]) $(id).disabled = on;
  if (!on) file.kind === "image" ? renderCrop() : renderTrim();
}

$("apply").addEventListener("click", async () => {
  if (media) media.pause();
  setBusy(true, t("Working…"));
  try {
    await invoke("editor_apply", { operation: operation() });
  } catch (e) {
    setBusy(false);
    $("status").textContent = String(e);
    $("status").classList.add("error");
  }
});
$("cancel").addEventListener("click", () => invoke("editor_close"));

listen("editor:progress", (e) => {
  $("status").textContent = `${t("Working…")} ${Math.round(e.payload * 100)} %`;
});
listen("editor:failed", (e) => {
  setBusy(false);
  $("status").textContent = e.payload;
  $("status").classList.add("error");
});
// A new file while the window is open: start over.
listen("editor:open", () => location.reload());

document.addEventListener("keydown", (e) => {
  if (e.target.tagName === "INPUT" || busy) return;
  if (e.key === "Escape") invoke("editor_close");
  if (e.key === "Enter" && !$("apply").disabled) $("apply").click();
  if (!media) return;
  if (e.key === "i") $("mark-start").click();
  if (e.key === "o") $("mark-end").click();
  if (e.key === " ") {
    e.preventDefault();
    media.paused ? media.play() : media.pause();
  }
});

(async () => {
  lang = await invoke("ui_language");
  translatePage();
  file = await invoke("editor_file");
  if (!file) return;
  $("name").textContent = file.name;
  if (file.kind === "image") setupCrop();
  else setupTrim();
})();
