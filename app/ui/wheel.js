// Draws the wheel from the segments computed in Rust (src/wheel.rs) and
// mirrors its hit-test for mouse / keyboard use in click mode.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

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

const C = 150; // centre, matches wheel.html and wheel::SIZE / 2
const GAP = 2.5; // half of the space between slices
const ROUND = 6; // corner radius
const HUB_TEXT = 60; // usable width inside the hub

let state = { segments: [], category: "convert", title: "", clickMode: false, hub: 42, busy: false, lang: "en" };
let active = -1;

const rad = (deg) => (deg * Math.PI) / 180;
const point = (r, deg) => [C + r * Math.sin(rad(deg)), C - r * Math.cos(rad(deg))];

// The sector shrunk by GAP + ROUND on every side; a round-joined stroke of
// width 2 × ROUND then grows it back with rounded corners.
function slicePath(s) {
  const pad = GAP + ROUND;
  const ri = s.r_in + pad, ro = s.r_out - pad;
  if (s.end - s.start >= 359.9) {
    return `M ${C} ${C - ro} A ${ro} ${ro} 0 1 1 ${C - 0.01} ${C - ro} Z ` +
      `M ${C} ${C - ri} A ${ri} ${ri} 0 1 0 ${C + 0.01} ${C - ri} Z`;
  }
  const deg = (r) => (pad / r) * (180 / Math.PI);
  const a0o = s.start + deg(ro), a1o = s.end - deg(ro);
  const a0i = s.start + deg(ri), a1i = s.end - deg(ri);
  const large = a1o - a0o > 180 ? 1 : 0;
  const [x0, y0] = point(ro, a0o), [x1, y1] = point(ro, a1o);
  const [x2, y2] = point(ri, a1i), [x3, y3] = point(ri, a0i);
  return `M ${x0} ${y0} A ${ro} ${ro} 0 ${large} 1 ${x1} ${y1} L ${x2} ${y2} A ${ri} ${ri} 0 ${large} 0 ${x3} ${y3} Z`;
}

// Long labels ("Extraire le texte") go on two lines, split at the space
// closest to the middle.
function splitLabel(label) {
  if (label.length <= 9 || !label.includes(" ")) return [label];
  let best = -1;
  for (let i = 0; i < label.length; i++) {
    if (label[i] === " " && (best < 0 || Math.abs(i - label.length / 2) < Math.abs(best - label.length / 2))) best = i;
  }
  return [label.slice(0, best), label.slice(best + 1)];
}

/// Font size at which a <text> (and its tspans) fits `width`, capped at `size`.
function fitSize(text, width, size = 10, min = 6.5) {
  text.style.fontSize = `${size}px`;
  const parts = text.querySelectorAll("tspan");
  const w = Math.max(...(parts.length ? [...parts] : [text]).map((p) => p.getComputedTextLength()));
  return w > width ? Math.max(min, (size * width) / w) : size;
}

function fit(text, width, size, min) {
  text.style.fontSize = `${fitSize(text, width, size, min)}px`;
}

function render() {
  slicesEl.replaceChildren();
  const sized = [];
  state.segments.forEach((s, i) => {
    const g = document.createElementNS(NS, "g");
    g.classList.add("slice", state.category);
    g.dataset.index = i;
    const d = slicePath(s);
    const shape = document.createElementNS(NS, "g");
    shape.setAttribute("class", "shape");
    for (const [cls, width] of [["edge", 2 * ROUND + 1.5], ["body", 2 * ROUND]]) {
      const path = document.createElementNS(NS, "path");
      path.setAttribute("class", cls);
      path.setAttribute("d", d);
      path.setAttribute("fill-rule", "evenodd");
      path.setAttribute("stroke-width", width);
      shape.append(path);
    }
    g.append(shape);
    const text = document.createElementNS(NS, "text");
    const full = s.end - s.start >= 359.9;
    const mid = full ? 0 : (s.start + s.end) / 2;
    const rm = (s.r_in + s.r_out) / 2;
    const [tx, ty] = point(rm, mid);
    text.setAttribute("x", tx);
    text.setAttribute("y", ty);
    text.setAttribute("text-anchor", "middle");
    const lines = splitLabel(s.label);
    if (lines.length === 1) {
      text.textContent = s.label;
    } else {
      lines.forEach((line, k) => {
        const span = document.createElementNS(NS, "tspan");
        span.setAttribute("x", tx);
        span.setAttribute("dy", k === 0 ? "-0.55em" : "1.15em");
        span.textContent = line;
        text.append(span);
      });
    }
    g.append(text);
    g.addEventListener("click", () => pick(i));
    slicesEl.append(g);
    // Chord of the slice at the label radius, minus the rounded padding.
    const step = full ? 180 : Math.min(s.end - s.start, 180);
    const chord = 2 * rm * Math.sin(rad(step / 2)) - 2 * (GAP + ROUND);
    sized.push([text, fitSize(text, 0.86 * Math.min(chord, s.r_out - s.r_in + 20))]);
  });
  // One size for the whole wheel, unless a label must go smaller to fit.
  const common = Math.max(8.5, Math.min(...sized.map(([, size]) => size)));
  for (const [text, size] of sized) text.style.fontSize = `${Math.min(common, size)}px`;
  setActive(-1);
  idleHub();
}

function hubText(title, sub = "") {
  hubSub.textContent = sub;
  const lines = sub ? [title] : splitLabel(title);
  hubTitle.replaceChildren();
  if (lines.length === 1) {
    hubTitle.textContent = title;
  } else {
    lines.forEach((line, k) => {
      const span = document.createElementNS(NS, "tspan");
      span.setAttribute("x", C);
      span.setAttribute("dy", k === 0 ? "-0.55em" : "1.15em");
      span.textContent = line;
      hubTitle.append(span);
    });
  }
  fit(hubTitle, HUB_TEXT);
  hubTitle.setAttribute("y", sub ? 146 : 150);
}

function idleHub() {
  hub.className.baseVal = "";
  hubProgress.style.strokeDasharray = "0 400";
  const name = state.title.length > 14 ? state.title.slice(0, 13) + "…" : state.title;
  hubText(t(state.category) || "", state.segments.length ? name : t("empty"));
}

function setActive(i) {
  active = i;
  slicesEl.querySelectorAll(".slice").forEach((g, j) => {
    g.classList.toggle("active", j === i);
  });
  if (state.busy) return;
  if (i === -2) {
    hub.className.baseVal = "cancel";
    hubText(t("cancel"));
  } else if (i >= 0 && state.segments[i]) {
    hub.className.baseVal = "";
    hubText(state.segments[i].label);
  } else {
    idleHub();
  }
}

// Highlight moved by the user in click mode: also tick the trackpad.
function userActive(i) {
  if (i === active) return;
  setActive(i);
  invoke("wheel_hover", { index: i });
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
  // Pending: open under the pointer but invisible until the drag brings files.
  if (payload.pending) svg.classList.remove("shown");
  else requestAnimationFrame(() => svg.classList.add("shown"));
});

listen("wheel:hide", () => {
  svg.classList.remove("shown");
  state.busy = false;
});

listen("wheel:hover", ({ payload }) => setActive(payload));

listen("wheel:status", ({ payload }) => {
  state.busy = payload.state === "working";
  slicesEl.querySelectorAll(".slice").forEach((g, j) => g.classList.toggle("dim", j !== active));
  const short = (m, n) => (m.length > n ? m.slice(0, n - 1) + "…" : m);
  if (payload.state === "working") {
    hub.className.baseVal = "working";
    hubText(payload.progress > 0 ? `${Math.round(payload.progress * 100)}%` : "…", short(payload.label, 14));
    const len = 2 * Math.PI * 39;
    hubProgress.style.strokeDasharray = `${len * payload.progress} ${len}`;
  } else if (payload.state === "done") {
    hub.className.baseVal = "done";
    hubText("✓", short(payload.message, 14));
  } else {
    hub.className.baseVal = "error";
    hubText(t("failed"), short(payload.message.replace(/^.*?: /, ""), 16));
  }
});

// --- Click mode: mouse and keyboard ------------------------------------------

document.addEventListener("mousemove", (e) => {
  if (state.clickMode && !state.busy) userActive(hitTest(e.clientX, e.clientY));
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
    case "ArrowRight": case "ArrowDown": if (n) userActive(active < 0 ? 0 : (active + 1) % n); break;
    case "ArrowLeft": case "ArrowUp": if (n) userActive(active < 0 ? n - 1 : (active - 1 + n) % n); break;
    case "Enter": case " ": if (active >= 0) pick(active); break;
    case "Tab": e.preventDefault(); switchCategory(e.shiftKey ? -1 : 1); break;
    case "1": case "2": case "3": invoke("wheel_category", { category: CATEGORIES[+e.key - 1] }); break;
    default: return;
  }
  e.preventDefault();
});
