// Draws the wheel from the segments computed in Rust (src/wheel.rs) and
// mirrors its hit-test for mouse / keyboard use in click mode.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const C = 210;
const NS = "http://www.w3.org/2000/svg";
const svg = document.getElementById("wheel");
const slicesEl = document.getElementById("slices");
const hub = document.getElementById("hub");
const hubTitle = document.getElementById("hub-title");
const hubSub = document.getElementById("hub-sub");
const hubProgress = document.getElementById("hub-progress");
const STRINGS = {
  en: { convert: "Convert", tool: "Tools", action: "Actions", cancel: "Cancel", failed: "Failed", empty: "Nothing to do" },
  fr: { convert: "Convertir", tool: "Outils", action: "Actions", cancel: "Annuler", failed: "Échec", empty: "Rien à faire" },
};
const t = (key) => (STRINGS[state.lang] || STRINGS.en)[key];
const CATEGORIES = ["convert", "tool", "action"];

let state = { segments: [], category: "convert", title: "", clickMode: false, hub: 56, busy: false, lang: "en" };
let active = -1;

const rad = (deg) => (deg * Math.PI) / 180;
const point = (r, deg) => [C + r * Math.sin(rad(deg)), C - r * Math.cos(rad(deg))];

function slicePath(s) {
  const gap = 1.6; // px between slices
  const full = s.end - s.start >= 359.9;
  if (full) {
    // A lone slice: draw an annulus.
    return `M ${C} ${C - s.r_out} A ${s.r_out} ${s.r_out} 0 1 1 ${C - 0.01} ${C - s.r_out} Z ` +
      `M ${C} ${C - s.r_in - gap} A ${s.r_in + gap} ${s.r_in + gap} 0 1 0 ${C + 0.01} ${C - s.r_in - gap} Z`;
  }
  const ri = s.r_in + gap, ro = s.r_out;
  const gi = (gap / ri) * (180 / Math.PI), go = (gap / ro) * (180 / Math.PI);
  const a0o = s.start + go, a1o = s.end - go, a0i = s.start + gi, a1i = s.end - gi;
  const large = s.end - s.start > 180 ? 1 : 0;
  const [x0, y0] = point(ro, a0o), [x1, y1] = point(ro, a1o);
  const [x2, y2] = point(ri, a1i), [x3, y3] = point(ri, a0i);
  return `M ${x0} ${y0} A ${ro} ${ro} 0 ${large} 1 ${x1} ${y1} L ${x2} ${y2} A ${ri} ${ri} 0 ${large} 0 ${x3} ${y3} Z`;
}

function render() {
  slicesEl.replaceChildren();
  state.segments.forEach((s, i) => {
    const g = document.createElementNS(NS, "g");
    g.classList.add("slice", state.category);
    if (s.label.length > 11) g.classList.add("small");
    g.dataset.index = i;
    const path = document.createElementNS(NS, "path");
    path.setAttribute("d", slicePath(s));
    path.setAttribute("fill-rule", "evenodd");
    const text = document.createElementNS(NS, "text");
    const mid = s.end - s.start >= 359.9 ? 0 : (s.start + s.end) / 2;
    const [tx, ty] = point((s.r_in + s.r_out) / 2 + 4, mid);
    text.setAttribute("x", tx);
    text.setAttribute("y", ty);
    text.setAttribute("text-anchor", "middle");
    text.textContent = s.label;
    g.append(path, text);
    g.addEventListener("click", () => pick(i));
    slicesEl.append(g);
  });
  setActive(-1);
  idleHub();
}

function idleHub() {
  hub.className.baseVal = "";
  hubProgress.style.strokeDasharray = "0 400";
  hubTitle.textContent = t(state.category) || "";
  const name = state.title.length > 16 ? state.title.slice(0, 15) + "…" : state.title;
  hubSub.textContent = state.segments.length ? name : t("empty");
}

function setActive(i) {
  active = i;
  slicesEl.querySelectorAll(".slice").forEach((g, j) => {
    g.classList.toggle("active", j === i);
  });
  if (state.busy) return;
  if (i === -2) {
    hub.className.baseVal = "cancel";
    hubTitle.textContent = t("cancel");
    hubSub.textContent = "";
  } else if (hub.className.baseVal === "cancel") {
    idleHub();
  }
}

function hitTest(x, y) {
  const dx = x - C, dy = y - C;
  const r = Math.hypot(dx, dy);
  if (r < state.hub) return -2;
  let a = (Math.atan2(dx, -dy) * 180) / Math.PI;
  a = ((a % 360) + 360) % 360;
  for (let i = 0; i < state.segments.length; i++) {
    const s = state.segments[i];
    if (r < s.r_in || r >= s.r_out) continue;
    const within = (v) => v >= s.start && v < s.end;
    if (within(a) || within(a - 360)) return i;
  }
  return -1;
}

function pick(i) {
  if (state.busy || i < 0) return;
  invoke("wheel_pick", { index: i });
}

function close() {
  if (!state.busy) invoke("wheel_close");
}

// --- Events from Rust -------------------------------------------------------

listen("wheel:show", ({ payload }) => {
  state = {
    segments: payload.segments,
    category: payload.category,
    title: payload.title,
    clickMode: payload.click_mode,
    hub: payload.hub,
    busy: false,
    lang: payload.lang,
  };
  render();
  requestAnimationFrame(() => svg.classList.add("shown"));
});

listen("wheel:hide", () => {
  svg.classList.remove("shown");
  state.busy = false;
});

listen("wheel:hover", ({ payload }) => setActive(payload));

listen("wheel:status", ({ payload }) => {
  state.busy = payload.state === "working";
  slicesEl.querySelectorAll(".slice").forEach((g, j) => g.classList.toggle("dim", j !== active));
  if (payload.state === "working") {
    hub.className.baseVal = "working";
    hubTitle.textContent = payload.progress > 0 ? `${Math.round(payload.progress * 100)}%` : "…";
    hubSub.textContent = payload.label;
    const len = 2 * Math.PI * 52;
    hubProgress.style.strokeDasharray = `${len * payload.progress} ${len}`;
  } else if (payload.state === "done") {
    hub.className.baseVal = "done";
    hubTitle.textContent = "✓";
    hubSub.textContent = payload.message.length > 16 ? payload.message.slice(0, 15) + "…" : payload.message;
  } else {
    hub.className.baseVal = "error";
    hubTitle.textContent = t("failed");
    const m = payload.message.replace(/^.*?: /, "");
    hubSub.textContent = m.length > 18 ? m.slice(0, 17) + "…" : m;
  }
});

// --- Click mode: mouse and keyboard ------------------------------------------

document.addEventListener("mousemove", (e) => {
  if (state.clickMode && !state.busy) setActive(hitTest(e.clientX, e.clientY));
});

document.addEventListener("mousedown", (e) => {
  if (!state.clickMode) return;
  const i = hitTest(e.clientX, e.clientY);
  if (i === -1 || i === -2) close();
});

window.addEventListener("blur", () => {
  if (state.clickMode) close();
});

function switchCategory(delta) {
  const i = CATEGORIES.indexOf(state.category);
  const next = CATEGORIES[(i + delta + CATEGORIES.length) % CATEGORIES.length];
  invoke("wheel_category", { category: next });
}

document.addEventListener("keydown", (e) => {
  if (state.busy) return;
  const n = state.segments.length;
  switch (e.key) {
    case "Escape": close(); break;
    case "ArrowRight": case "ArrowDown": if (n) setActive(active < 0 ? 0 : (active + 1) % n); break;
    case "ArrowLeft": case "ArrowUp": if (n) setActive(active < 0 ? n - 1 : (active - 1 + n) % n); break;
    case "Enter": case " ": if (active >= 0) pick(active); break;
    case "Tab": e.preventDefault(); switchCategory(e.shiftKey ? -1 : 1); break;
    case "1": case "2": case "3": invoke("wheel_category", { category: CATEGORIES[+e.key - 1] }); break;
    default: return;
  }
  e.preventDefault();
});
