const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const list = document.getElementById("list");

function ext(name) {
  const i = name.lastIndexOf(".");
  return i > 0 ? name.slice(i + 1, i + 5) : "file";
}

async function refresh() {
  const items = await invoke("float_items");
  list.replaceChildren();
  document.getElementById("empty").hidden = items.length > 0;
  items.forEach((item, i) => {
    const li = document.createElement("li");
    li.title = item.path;
    const badge = document.createElement("span");
    badge.className = "badge";
    badge.textContent = item.kind === "folder" ? "dir" : ext(item.name);
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = item.name;
    const wheel = document.createElement("button");
    wheel.textContent = "◎";
    wheel.title = "Open the wheel for this file";
    wheel.addEventListener("mousedown", (e) => e.stopPropagation());
    wheel.addEventListener("click", () => invoke("float_open_wheel", { index: i }));
    const remove = document.createElement("button");
    remove.textContent = "×";
    remove.title = "Remove from Float";
    remove.addEventListener("mousedown", (e) => e.stopPropagation());
    remove.addEventListener("click", async () => { await invoke("float_remove", { index: i }); refresh(); });
    li.append(badge, name, wheel, remove);
    li.addEventListener("mousedown", (e) => { if (e.button === 0) invoke("float_drag", { index: i }); });
    list.append(li);
  });
}

document.getElementById("drag-all").addEventListener("mousedown", () => invoke("float_drag", { index: null }));
document.getElementById("clear").addEventListener("click", async () => { await invoke("float_remove", { index: null }); refresh(); });
listen("float:update", refresh);
window.addEventListener("focus", refresh);
refresh();
