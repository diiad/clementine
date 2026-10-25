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
  "Built in": "Intégré",
  "Not installed": "Non installé",
  "Press keys…": "Tapez les touches…",
  "Add ⌘, ⌥ or ⌃": "Ajoutez ⌘, ⌥ ou ⌃",
  "Add Ctrl, Alt or Win": "Ajoutez Ctrl, Alt ou Win",
  "None": "Aucun",
  "Version": "Version",
  "MIT licensed": "licence MIT",
  "Video preset": "Préréglage vidéo",
  "Standard (follows quality)": "Standard (suit la qualité)",
  "Web (≤ 1080p, lighter)": "Web (≤ 1080p, plus léger)",
  "Max (near lossless)": "Max (quasi sans perte)",
  "“Fit size” target": "Taille visée par « ≤ … Mo »",
  "8 MB": "8 Mo",
  "10 MB (Discord)": "10 Mo (Discord)",
  "25 MB (email)": "25 Mo (e-mail)",
  "50 MB": "50 Mo",
  "100 MB": "100 Mo",
  "Recipes": "Recettes",
  "Chain several steps into one slice, shown first on the Tools wheel.":
    "Enchaînez plusieurs étapes dans une seule tranche, en tête de la roue Outils.",
  "Recipe name": "Nom de la recette",
  "Add a step…": "Ajouter une étape…",
  "Pick steps from the menu.": "Choisissez des étapes dans le menu.",
  "Add recipe": "Ajouter",
  "Save": "Enregistrer",
  "Cancel": "Annuler",
  "Edit": "Modifier",
  "Delete": "Supprimer",
  "Remove": "Retirer",
  "Unknown step": "Étape inconnue",
  "Images": "Images",
  "Video": "Vidéo",
  "Audio": "Audio",
  "PDF": "PDF",
  "Documents": "Documents",
  "Spreadsheets": "Tableurs",
  "Subtitles": "Sous-titres",
  "Archives": "Archives",
  "Any file": "Tout fichier",
};
let lang = "en";
let platform = "macos";
const t = (en) => (lang === "fr" && FR[en]) || en;

// Windows wording for the few macOS-specific strings, applied before
// translation (FR has entries for both).
const WINDOWS_TEXT = {
  "Drag a file, hold a key, drop it on the wheel. Everything runs on this Mac.":
    "Drag a file, hold a key, drop it on the wheel. Everything runs on this PC.",
  "Hold one of these while dragging files.":
    "Hold one of these while dragging files from the desktop or File Explorer.",
  "Wheel for Finder selection": "Wheel for Explorer selection",
  "Reveal results in Finder": "Show results in File Explorer",
  "Hide menu bar icon": "Hide tray icon",
  "Open Clémentine again from Finder or Spotlight to get back here.":
    "Open Clémentine again from the Start menu to get back here.",
  "Click, then press the new shortcut. ⌫ clears it, Esc cancels.":
    "Click, then press the new shortcut. Backspace clears it, Esc cancels.",
};
Object.assign(FR, {
  "Restart": "Redémarrer",
  "Installed. Restart Clémentine to use it.": "Installé. Redémarrez Clémentine pour l’utiliser.",
  "Check for updates automatically": "Rechercher les mises à jour automatiquement",
  "Check now": "Vérifier",
  "Checking…": "Vérification…",
  "Version {v} is available.": "La version {v} est disponible.",
  "Clémentine is up to date.": "Clémentine est à jour.",
  "Could not check for updates:": "Impossible de vérifier les mises à jour :",
  "Install": "Installer",
  "Installing…": "Installation…",
  "Installation failed:": "Échec de l’installation :",
  "winget is missing: the download page opened in your browser.":
    "winget est absent : la page de téléchargement s’est ouverte dans le navigateur.",
  "Homebrew is missing: the download page opened in your browser.":
    "Homebrew est absent : la page de téléchargement s’est ouverte dans le navigateur.",
  "Drag a file, hold a key, drop it on the wheel. Everything runs on this PC.":
    "Glissez un fichier, maintenez une touche, lâchez-le sur la roue. Tout se passe sur ce PC.",
  "Hold one of these while dragging files from the desktop or File Explorer.":
    "Maintenez l’une d’elles pendant que vous glissez des fichiers depuis le bureau ou l’Explorateur.",
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
  const winKeys = { shift: "Shift", command: "Win", control: "Ctrl", option: "Alt" };
  for (const id of ["convert_key", "tools_key", "actions_key"]) {
    for (const o of $(id).options) o.textContent = winKeys[o.value];
  }
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
  for (const id of ["enabled", "reveal_after", "notify", "haptics", "hide_menu_bar_icon", "auto_update"]) $(id).checked = settings[id];
  $("hidden-hint").hidden = !settings.hide_menu_bar_icon;
  $("custom_dir").textContent = settings.custom_dir || t("No folder chosen");
  $("language").value = settings.language;
  $("haptic_strength").value = settings.haptic_strength;
  $("strength-row").hidden = !settings.haptics;
  $("custom-row").hidden = settings.output !== "custom";
  $("video_preset").value = settings.video_preset;
  $("target_mb").value = String(settings.target_mb);
  renderRecipes();
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
bind("video_preset", (el) => el.value);
$("target_mb").addEventListener("change", async (e) => {
  settings.target_mb = Number(e.target.value);
  await save();
  loadCatalog(); // relabels "≤ N MB"
});
bind("quality", (el) => Number(el.value));
bind("enabled", (el) => el.checked);
bind("reveal_after", (el) => el.checked);
bind("notify", (el) => el.checked);
bind("haptics", (el) => el.checked);
bind("hide_menu_bar_icon", (el) => el.checked);
bind("auto_update", (el) => el.checked);
$("check-update").addEventListener("click", async () => {
  const button = $("check-update");
  const note = $("update-status");
  button.disabled = true;
  note.textContent = t("Checking…");
  note.hidden = false;
  try {
    const version = await invoke("check_update");
    note.textContent = version
      ? t("Version {v} is available.").replace("{v}", version)
      : t("Clémentine is up to date.");
  } catch (e) {
    note.textContent = `${t("Could not check for updates:")} ${e}`;
  }
  button.disabled = false;
});
bind("haptic_strength", (el) => el.value);
$("haptic_strength").addEventListener("change", (e) => invoke("test_haptic", { strength: e.target.value }));
$("language").addEventListener("change", async (e) => {
  settings.language = e.target.value;
  await save();
  lang = await invoke("ui_language");
  translatePage();
  fill();
  refreshEngines();
  loadCatalog();
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

const installing = new Set();

function status(id, path, bundled = false) {
  const el = $(id);
  el.textContent = t(bundled ? "Built in" : path ? "Found" : "Not installed");
  el.title = path || "";
  el.className = "status " + (path ? "ok" : "missing");
  if (!installing.has(id)) $(`install-${id}`).hidden = !!path;
}

for (const id of ["ffmpeg", "libreoffice"]) {
  const button = $(`install-${id}`);
  button.addEventListener("click", async () => {
    installing.add(id);
    button.disabled = true;
    button.textContent = t("Installing…");
    $("install-error").hidden = true;
    try {
      const outcome = await invoke("install_engine", { name: id });
      if (outcome === "opened") {
        $("install-error").textContent = t(
          platform === "windows"
            ? "winget is missing: the download page opened in your browser."
            : "Homebrew is missing: the download page opened in your browser.",
        );
        $("install-error").hidden = false;
      }
    } catch (e) {
      $("install-error").textContent = `${t("Installation failed:")} ${e}`;
      $("install-error").hidden = false;
    }
    installing.delete(id);
    button.disabled = false;
    button.textContent = t("Install");
    const engines = await refreshEngines();
    // Installed but not visible to this process yet: offer a restart.
    if (!$("install-error").hidden || engines[id]) return;
    $("restart-note").textContent = t("Installed. Restart Clémentine to use it.");
    $("restart-row").hidden = false;
  });
}

$("restart").addEventListener("click", () => invoke("restart_app"));

// --- Recipes -------------------------------------------------------------------
// A recipe is a name and a list of operation ids, run in order by the core.

const FAMILIES = {
  image: "Images", video: "Video", audio: "Audio", pdf: "PDF", document: "Documents",
  spreadsheet: "Spreadsheets", subtitle: "Subtitles", archive: "Archives", any: "Any file",
};
let catalog = [];
const labels = new Map(); // operation id → label
let draft = { name: "", steps: [] };
let editing = -1; // index of the recipe in the editor, -1 for a new one

const stepLabel = (id) => {
  const label = labels.get(id);
  if (!label) return `${t("Unknown step")} (${id})`;
  return id.startsWith("convert:") ? `→ ${label}` : label;
};

async function loadCatalog() {
  catalog = await invoke("operation_catalog");
  labels.clear();
  const select = $("recipe-step");
  select.replaceChildren(new Option(t("Add a step…"), ""));
  for (const { family, ops } of catalog) {
    const group = document.createElement("optgroup");
    group.label = t(FAMILIES[family] || family);
    for (const op of ops) {
      if (!labels.has(op.id)) labels.set(op.id, op.label);
      group.append(new Option(stepLabel(op.id), op.id));
    }
    select.append(group);
  }
  renderRecipes();
}

function stepsView(steps, removable) {
  const box = document.createElement("span");
  box.className = "steps";
  steps.forEach((id, i) => {
    if (i) box.append(Object.assign(document.createElement("span"), { className: "arrow", textContent: "›" }));
    const chip = Object.assign(document.createElement("span"), { className: "chip", textContent: stepLabel(id) });
    if (removable) {
      const x = Object.assign(document.createElement("button"), { textContent: "×", title: t("Remove") });
      x.addEventListener("click", () => {
        draft.steps.splice(i, 1);
        renderDraft();
      });
      chip.append(x);
    }
    box.append(chip);
  });
  return box;
}

function renderRecipes() {
  if (!settings) return;
  const list = $("recipe-list");
  list.replaceChildren();
  settings.recipes.forEach((r, i) => {
    const row = Object.assign(document.createElement("div"), { className: "row recipe" });
    const name = Object.assign(document.createElement("span"), { className: "name", textContent: r.name });
    const edit = Object.assign(document.createElement("button"), { className: "link", textContent: t("Edit") });
    edit.addEventListener("click", () => {
      editing = i;
      draft = { name: r.name, steps: [...r.steps] };
      renderDraft();
      $("recipe-name").focus();
    });
    const del = Object.assign(document.createElement("button"), { className: "link", textContent: t("Delete") });
    del.addEventListener("click", () => {
      settings.recipes.splice(i, 1);
      if (editing === i) resetDraft();
      else if (editing > i) editing--;
      save();
    });
    const buttons = Object.assign(document.createElement("span"), { className: "engine" });
    buttons.append(edit, del);
    row.append(name, stepsView(r.steps, false), buttons);
    list.append(row);
  });
  renderDraft();
}

function renderDraft() {
  $("recipe-name").value = draft.name;
  $("recipe-name").placeholder = t("Recipe name");
  const steps = draft.steps.length
    ? stepsView(draft.steps, true)
    : Object.assign(document.createElement("span"), { className: "steps", textContent: t("Pick steps from the menu.") });
  steps.id = "recipe-steps";
  $("recipe-steps").replaceWith(steps);
  $("recipe-save").textContent = t(editing < 0 ? "Add recipe" : "Save");
  $("recipe-save").disabled = !draft.name.trim() || !draft.steps.length;
  $("recipe-cancel").hidden = editing < 0;
}

function resetDraft() {
  editing = -1;
  draft = { name: "", steps: [] };
  renderDraft();
}

$("recipe-name").addEventListener("input", (e) => {
  draft.name = e.target.value;
  $("recipe-save").disabled = !draft.name.trim() || !draft.steps.length;
});
$("recipe-step").addEventListener("change", (e) => {
  const id = e.target.value;
  e.target.value = "";
  if (!id) return;
  // An action (Copy…) can only end a recipe.
  if (draft.steps.some((s) => s.startsWith("action:"))) draft.steps.pop();
  draft.steps.push(id);
  renderDraft();
});
$("recipe-cancel").addEventListener("click", resetDraft);
$("recipe-save").addEventListener("click", () => {
  const recipe = { name: draft.name.trim(), steps: draft.steps };
  if (editing < 0) settings.recipes.push(recipe);
  else settings.recipes[editing] = recipe;
  editing = -1;
  draft = { name: "", steps: [] };
  save();
});

async function refreshEngines() {
  const engines = await invoke("engine_status");
  if (engines.ffmpeg && engines.libreoffice) $("restart-row").hidden = true;
  status("ffmpeg", engines.ffmpeg, engines.ffmpeg_bundled);
  status("libreoffice", engines.libreoffice);
  $("autostart").checked = engines.autostart;
  $("version").textContent = `${t("Version")} ${engines.version} · ${t("MIT licensed")}`;
  return engines;
}

(async () => {
  settings = await invoke("get_settings");
  platform = (await invoke("engine_status")).platform || "macos";
  adaptToPlatform();
  lang = await invoke("ui_language");
  translatePage();
  fill();
  refreshEngines();
  loadCatalog();
})();
