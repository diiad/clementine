const { invoke } = window.__TAURI__.core;

const KEYS = [
  ["shift", "⇧ Shift"],
  ["command", "⌘ Command"],
  ["control", "⌃ Control"],
  ["option", "⌥ Option"],
];
const $ = (id) => document.getElementById(id);
let settings = null;

for (const id of ["convert_key", "tools_key", "actions_key"]) {
  for (const [value, label] of KEYS) {
    $(id).append(new Option(label, value));
  }
}

function fill() {
  for (const id of ["convert_key", "tools_key", "actions_key", "output"]) $(id).value = settings[id];
  if (!recording) $("shortcut").textContent = prettyShortcut(settings.shortcut);
  $("dpi").value = String(settings.dpi);
  $("quality").value = settings.quality;
  $("quality-val").textContent = settings.quality;
  for (const id of ["enabled", "reveal_after", "notify"]) $(id).checked = settings[id];
  $("custom_dir").textContent = settings.custom_dir || "No folder chosen";
  $("custom-row").hidden = settings.output !== "custom";
  const keys = [settings.convert_key, settings.tools_key, settings.actions_key];
  $("key-warning").hidden = new Set(keys).size === keys.length;
}

async function save() {
  $("shortcut-error").hidden = true;
  try {
    await invoke("save_settings", { settings });
  } catch (e) {
    // Rejected: reload what is actually in effect.
    settings = await invoke("get_settings");
    $("shortcut-error").textContent = String(e);
    $("shortcut-error").hidden = false;
  }
  fill();
}

// --- Shortcut recorder ---------------------------------------------------------
// Stored in the global-shortcut plugin's syntax ("Alt+Super+KeyT"), shown
// with Mac symbols ("⌥⌘T").

const MOD_SYMBOLS = { Control: "⌃", Alt: "⌥", Shift: "⇧", Super: "⌘" };
const KEY_SYMBOLS = {
  Space: "Space", Enter: "↩", Backspace: "⌫", Tab: "⇥", Escape: "⎋",
  ArrowUp: "↑", ArrowDown: "↓", ArrowLeft: "←", ArrowRight: "→",
  Minus: "-", Equal: "=", Comma: ",", Period: ".", Slash: "/", Semicolon: ";",
  Quote: "'", BracketLeft: "[", BracketRight: "]", Backslash: "\\", Backquote: "`",
};
let recording = false;

function keyLabel(code) {
  if (KEY_SYMBOLS[code]) return KEY_SYMBOLS[code];
  return code.replace(/^Key/, "").replace(/^Digit/, "").replace(/^Numpad/, "Num ");
}

function prettyShortcut(s) {
  if (!s) return "None";
  const parts = s.split("+");
  const key = parts.pop();
  // Mac order: ⌃⌥⇧⌘.
  const mods = ["Control", "Alt", "Shift", "Super"].filter((m) => parts.includes(m));
  return mods.map((m) => MOD_SYMBOLS[m]).join("") + keyLabel(key);
}

function stopRecording() {
  recording = false;
  $("shortcut").classList.remove("recording");
  fill();
}

$("shortcut").addEventListener("click", () => {
  recording = true;
  $("shortcut").classList.add("recording");
  $("shortcut").textContent = "Press keys…";
});
$("shortcut").addEventListener("blur", () => recording && stopRecording());

document.addEventListener("keydown", (e) => {
  if (!recording) return;
  e.preventDefault();
  if (["Shift", "Control", "Alt", "Meta"].includes(e.key)) return; // wait for the key itself
  const mods = [];
  if (e.ctrlKey) mods.push("Control");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");
  if (!mods.length && e.code === "Escape") return stopRecording();
  if (!mods.length && e.code === "Backspace") {
    settings.shortcut = "";
  } else if (!mods.length || (mods.length === 1 && mods[0] === "Shift")) {
    $("shortcut").textContent = "Add ⌘, ⌥ or ⌃";
    return;
  } else {
    settings.shortcut = [...mods, e.code].join("+");
  }
  recording = false;
  $("shortcut").classList.remove("recording");
  save();
});

function bind(id, read) {
  $(id).addEventListener("change", () => {
    settings[id] = read($(id));
    save();
  });
}

bind("convert_key", (el) => el.value);
bind("tools_key", (el) => el.value);
bind("actions_key", (el) => el.value);
bind("output", (el) => el.value);
bind("dpi", (el) => Number(el.value));
bind("quality", (el) => Number(el.value));
bind("enabled", (el) => el.checked);
bind("reveal_after", (el) => el.checked);
bind("notify", (el) => el.checked);
$("quality").addEventListener("input", (e) => ($("quality-val").textContent = e.target.value));

$("choose").addEventListener("click", async () => {
  const dir = await invoke("choose_folder");
  if (dir) {
    settings.custom_dir = dir;
    save();
  }
});

$("autostart").addEventListener("change", (e) => invoke("set_autostart", { enabled: e.target.checked }));

function status(id, path) {
  const el = $(id);
  el.textContent = path ? "Found" : "Not installed";
  el.title = path || "";
  el.className = "status " + (path ? "ok" : "missing");
}

(async () => {
  settings = await invoke("get_settings");
  fill();
  const engines = await invoke("engine_status");
  status("ffmpeg", engines.ffmpeg);
  status("libreoffice", engines.libreoffice);
  $("autostart").checked = engines.autostart;
  $("version").textContent = `Version ${engines.version} · MIT licensed`;
})();
