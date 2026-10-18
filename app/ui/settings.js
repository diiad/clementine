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
  for (const id of ["convert_key", "tools_key", "actions_key", "output", "shortcut"]) $(id).value = settings[id];
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
  try {
    await invoke("save_settings", { settings });
  } catch (e) {
    console.error(e);
  }
  fill();
}

function bind(id, read) {
  $(id).addEventListener("change", () => {
    settings[id] = read($(id));
    save();
  });
}

bind("convert_key", (el) => el.value);
bind("tools_key", (el) => el.value);
bind("actions_key", (el) => el.value);
bind("shortcut", (el) => el.value.trim());
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
