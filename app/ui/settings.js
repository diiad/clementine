const { invoke } = window.__TAURI__.core;

const KEYS = [
  ["shift", "⇧ Shift"],
  ["command", "⌘ Command"],
  ["control", "⌃ Control"],
  ["option", "⌥ Option"],
];
const $ = (id) => document.getElementById(id);
let settings = null;

// --- Translation ---------------------------------------------------------------
// The page is written in English; French replaces text nodes that match.

const FR = {
  "Drag a file, hold a key, drop it on the wheel. Everything runs on this Mac.":
    "Glissez un fichier, maintenez une touche, lâchez-le sur la roue. Tout se passe sur ce Mac.",
  "Keys": "Touches",
  "Hold one of these while dragging files.": "Maintenez l’une d’elles pendant que vous glissez des fichiers.",
  "Convert": "Convertir",
  "Tools": "Outils",
  "Actions": "Actions",
  "Wheel for Finder selection": "Roue pour la sélection du Finder",
  "Click, then press the new shortcut. ⌫ clears it, Esc cancels.":
    "Cliquez, puis tapez le nouveau raccourci. ⌫ l’efface, Échap annule.",
  "Each wheel needs its own key.": "Chaque roue doit avoir sa propre touche.",
  "Output": "Résultats",
  "Save results": "Enregistrer",
  "Next to the original": "À côté de l’original",
  "In Downloads": "Dans Téléchargements",
  "In a folder…": "Dans un dossier…",
  "No folder chosen": "Aucun dossier choisi",
  "Choose…": "Choisir…",
  "Quality": "Qualité",
  "PDF & SVG resolution": "Résolution PDF et SVG",
  "Reveal results in Finder": "Afficher les résultats dans le Finder",
  "Notify when long jobs finish": "Notifier à la fin des tâches longues",
  "Haptic feedback on the trackpad": "Retour haptique sur le trackpad",
  "Haptic strength": "Intensité",
  "Light": "Léger",
  "Medium": "Moyen",
  "Strong": "Fort",
  "General": "Général",
  "Enabled": "Activé",
  "Open at login": "Ouvrir au démarrage",
  "Language": "Langue",
  "Hide menu bar icon": "Masquer l’icône de la barre des menus",
  "Open Clémentine again from Finder or Spotlight to get back here.":
    "Rouvrez Clémentine depuis le Finder ou Spotlight pour revenir ici.",
  "System": "Système",
  "Engines": "Moteurs",
  "ffmpeg (audio & video)": "ffmpeg (audio et vidéo)",
  "LibreOffice (slides, better Office → PDF)": "LibreOffice (présentations, meilleur Office → PDF)",
  "Found": "Installé",
  "Not installed": "Non installé",
  "Press keys…": "Tapez les touches…",
  "Add ⌘, ⌥ or ⌃": "Ajoutez ⌘, ⌥ ou ⌃",
  "Add Ctrl, Alt or Win": "Ajoutez Ctrl, Alt ou Win",
  "None": "Aucun",
  "Version": "Version",
  "MIT licensed": "licence MIT",
};
let lang = "en";
let platform = "macos";
const t = (en) => (lang === "fr" && FR[en]) || en;

// Windows wording for the few macOS-specific strings, applied before
// translation (FR has entries for both).
const WINDOWS_TEXT = {
  "Wheel for Finder selection": "Wheel for Explorer selection",
  "Reveal results in Finder": "Show results in File Explorer",
  "Hide menu bar icon": "Hide tray icon",
  "Open Clémentine again from Finder or Spotlight to get back here.":
    "Open Clémentine again from the Start menu to get back here.",
  "Click, then press the new shortcut. ⌫ clears it, Esc cancels.":
    "Click, then press the new shortcut. Backspace clears it, Esc cancels.",
};
Object.assign(FR, {
  "Wheel for Explorer selection": "Roue pour la sélection de l’Explorateur",
  "Show results in File Explorer": "Afficher les résultats dans l’Explorateur",
  "Hide tray icon": "Masquer l’icône de la zone de notification",
  "Open Clémentine again from the Start menu to get back here.":
    "Rouvrez Clémentine depuis le menu Démarrer pour revenir ici.",
  "Click, then press the new shortcut. Backspace clears it, Esc cancels.":
    "Cliquez, puis tapez le nouveau raccourci. Retour arrière l’efface, Échap annule.",
});

function adaptToPlatform() {
  if (platform !== "windows") return;
  document.body.classList.add("windows");
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    const key = n.nodeValue.trim();
    if (WINDOWS_TEXT[key]) n.nodeValue = n.nodeValue.replace(key, WINDOWS_TEXT[key]);
  }
  for (const el of document.querySelectorAll("[title]")) {
    if (WINDOWS_TEXT[el.title]) el.title = WINDOWS_TEXT[el.title];
  }
}
const original = new WeakMap();

function translatePage() {
  document.documentElement.lang = lang;
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    if (!original.has(n)) original.set(n, n.nodeValue);
    const en = original.get(n);
    const key = en.trim();
    if (key && FR[key] !== undefined) n.nodeValue = en.replace(key, t(key));
  }
  for (const el of document.querySelectorAll("[title]")) {
    if (!el.dataset.title) el.dataset.title = el.title;
    el.title = t(el.dataset.title);
  }
}

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
  for (const id of ["enabled", "reveal_after", "notify", "haptics", "hide_menu_bar_icon"]) $(id).checked = settings[id];
  $("hidden-hint").hidden = !settings.hide_menu_bar_icon;
  $("custom_dir").textContent = settings.custom_dir || t("No folder chosen");
  $("language").value = settings.language;
  $("haptic_strength").value = settings.haptic_strength;
  $("strength-row").hidden = !settings.haptics;
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
  if (!s) return t("None");
  const parts = s.split("+");
  const key = parts.pop();
  if (platform === "windows") {
    const names = { Control: "Ctrl", Alt: "Alt", Shift: "Shift", Super: "Win" };
    const mods = ["Control", "Alt", "Shift", "Super"].filter((m) => parts.includes(m));
    return [...mods.map((m) => names[m]), keyLabel(key)].join("+");
  }
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
  $("shortcut").textContent = t("Press keys…");
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
    $("shortcut").textContent = t(platform === "windows" ? "Add Ctrl, Alt or Win" : "Add ⌘, ⌥ or ⌃");
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
bind("haptics", (el) => el.checked);
bind("hide_menu_bar_icon", (el) => el.checked);
bind("haptic_strength", (el) => el.value);
$("haptic_strength").addEventListener("change", (e) => invoke("test_haptic", { strength: e.target.value }));
$("language").addEventListener("change", async (e) => {
  settings.language = e.target.value;
  await save();
  lang = await invoke("ui_language");
  translatePage();
  fill();
  refreshEngines();
});
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
  el.textContent = t(path ? "Found" : "Not installed");
  el.title = path || "";
  el.className = "status " + (path ? "ok" : "missing");
}

async function refreshEngines() {
  const engines = await invoke("engine_status");
  status("ffmpeg", engines.ffmpeg);
  status("libreoffice", engines.libreoffice);
  $("autostart").checked = engines.autostart;
  $("version").textContent = `${t("Version")} ${engines.version} · ${t("MIT licensed")}`;
}

(async () => {
  settings = await invoke("get_settings");
  platform = (await invoke("engine_status")).platform || "macos";
  adaptToPlatform();
  lang = await invoke("ui_language");
  translatePage();
  fill();
  refreshEngines();
})();
