"use strict";

// Filled in by the installer. The fallback lets the page be previewed in a browser.
const K = window.__KIWI__ || {
  mode: new URLSearchParams(location.search).get("mode") || "install",
  version: "0.1.0",
  dir: "C:\\Users\\you\\AppData\\Local\\Programs\\KiwiConvert",
  existing: null,
  sizeMb: 238,
};

const $ = (s) => document.querySelector(s);
const $$ = (s) => [...document.querySelectorAll(s)];
const post = (cmd, data = {}) => window.ipc?.postMessage(JSON.stringify({ cmd, ...data }));
const installing = K.mode === "install";
const updating = installing && K.existing;

const state = {
  dir: K.dir,
  desktopShortcut: true,
  startWithWindows: true,
  removeData: false,
  busy: false,
};

// ---- Screens -----------------------------------------------------------------------

let current = null;

function show(id) {
  const next = document.getElementById(id);
  if (next === current) return;
  if (current) {
    const old = current;
    old.classList.remove("active");
    old.classList.add("leaving");
    setTimeout(() => old.classList.remove("leaving"), 600);
  }
  [...next.children].forEach((child, i) => child.style.setProperty("--i", i));
  next.classList.remove("leaving");
  next.classList.add("active");
  current = next;
  const primary = next.querySelector("[data-primary]");
  if (primary) setTimeout(() => primary.focus({ preventScroll: true }), 400);
}

function setBusy(busy) {
  state.busy = busy;
  document.body.classList.toggle("busy", busy);
}

// ---- Sheets ------------------------------------------------------------------------

let sheet = null;

function openSheet(id) {
  sheet = document.getElementById(id);
  sheet.classList.add("open");
  $("#backdrop").classList.add("open");
}

function closeSheet() {
  if (!sheet) return;
  sheet.classList.remove("open");
  $("#backdrop").classList.remove("open");
  sheet = null;
}

// ---- The kiwi progress ring --------------------------------------------------------

const NS = "http://www.w3.org/2000/svg";

function svg(tag, attrs = {}, parent) {
  const node = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, v);
  if (parent) parent.appendChild(node);
  return node;
}

function polar(r, deg) {
  const a = (deg * Math.PI) / 180;
  return `${(100 + r * Math.cos(a)).toFixed(2)} ${(100 + r * Math.sin(a)).toFixed(2)}`;
}

function gradient(defs, id, stops, attrs = {}) {
  const g = svg("radialGradient", { id, ...attrs }, defs);
  for (const [offset, color] of stops) svg("stop", { offset, "stop-color": color }, g);
}

function buildRing(host) {
  const root = svg("svg", { viewBox: "0 0 200 200", class: "ring" }, host);
  const defs = svg("defs", {}, root);
  gradient(defs, "skin", [["0.86", "#7a5631"], ["1", "#3d2914"]]);
  gradient(defs, "flesh", [["0.3", "#e9f8b4"], ["0.62", "#a9d94c"], ["1", "#6aa12a"]]);
  gradient(defs, "core", [["0", "#fffdf0"], ["1", "#eef0c8"]]);

  svg("circle", { cx: 100, cy: 100, r: 97, fill: "url(#skin)", stroke: "none" }, root);
  svg("circle", { cx: 100, cy: 100, r: 90, fill: "#1b2418", stroke: "none" }, root);

  // Twelve slices light up one after another as the work advances.
  const wedges = [];
  for (let i = 0; i < 12; i++) {
    const a0 = -90 + i * 30 + 0.9;
    const a1 = -90 + (i + 1) * 30 - 0.9;
    const d = `M ${polar(89, a0)} A 89 89 0 0 1 ${polar(89, a1)} L ${polar(31, a1)} A 31 31 0 0 0 ${polar(31, a0)} Z`;
    const g = svg("g", { class: "wedge" }, root);
    svg("path", { d, fill: "url(#flesh)", stroke: "none" }, g);
    // The pale rays of a real slice.
    svg("path", { d: `M ${polar(34, a0 + 14)} L ${polar(70, a0 + 14)}`, stroke: "rgb(255 255 235 / 0.45)", "stroke-width": 1.4 }, g);
    wedges.push(g);
  }

  const seeds = svg("g", { class: "seeds" }, root);
  for (let i = 0; i < 22; i++) {
    const deg = (360 / 22) * i;
    const r = 43 + (i % 2) * 5;
    const [x, y] = polar(r, deg).split(" ");
    svg("ellipse", { cx: x, cy: y, rx: 1.9, ry: 3.8, fill: "#1f1a12", stroke: "none", transform: `rotate(${deg + 90} ${x} ${y})` }, seeds);
  }

  svg("circle", { cx: 100, cy: 100, r: 30, fill: "url(#core)", stroke: "none" }, root);
  const circumference = 2 * Math.PI * 93.5;
  const arc = svg("circle", { cx: 100, cy: 100, r: 93.5, class: "arc", "stroke-dasharray": circumference.toFixed(1) }, root);
  const pct = svg("text", { x: 100, y: 108.5, class: "pct" }, root);
  svg("path", { d: "M 87 101 L 96 110 L 114 91", class: "check" }, root);

  const api = {
    set(p) {
      wedges.forEach((w, i) => {
        const v = Math.min(1, Math.max(0, p * 12 - i));
        w.style.opacity = String(v);
        w.style.transform = `scale(${0.9 + 0.1 * v})`;
      });
      seeds.style.opacity = String(Math.min(1, 0.25 + p * 1.5));
      arc.style.strokeDashoffset = String(circumference * (1 - p));
      pct.textContent = `${Math.round(p * 100)}%`;
    },
    reset() {
      root.classList.remove("done");
      api.set(0);
    },
    done() {
      root.classList.add("done");
    },
  };
  return api;
}

function burst() {
  const host = $("#burst");
  host.replaceChildren();
  for (let i = 0; i < 16; i++) {
    const seed = document.createElement("i");
    seed.style.setProperty("--a", `${(360 / 16) * i + Math.random() * 10}deg`);
    seed.style.setProperty("--r", `${110 + Math.random() * 40}px`);
    seed.style.animationDelay = `${Math.random() * 80}ms`;
    host.appendChild(seed);
  }
}

// ---- Progress ----------------------------------------------------------------------

const ring = buildRing($("#ring"));
let target = 0;
let shown = 0;
let startedAt = 0;
let lastFrame = 0;
let outcome;
let notes = [];
let frame = 0;

function startProgress() {
  target = 0;
  shown = 0;
  outcome = undefined;
  startedAt = lastFrame = performance.now();
  $("#progress-title").textContent = installing
    ? updating
      ? "Updating KiwiConvert"
      : "Installing KiwiConvert"
    : "Removing KiwiConvert";
  $("#progress-file").textContent = "Getting ready";
  ring.reset();
  show("progress");
  setBusy(true);
  cancelAnimationFrame(frame);
  frame = requestAnimationFrame(tick);
}

// The work often takes a second or two, so the ring eases toward the real progress
// instead of jumping, and stays up long enough to be read.
function tick(now) {
  const dt = Math.min(0.1, (now - lastFrame) / 1000);
  lastFrame = now;
  const gap = target - shown;
  if (gap > 0) shown += Math.max(gap * (1 - Math.exp(-dt / 0.22)), Math.min(gap, 0.15 * dt));
  ring.set(shown);
  $("#progress-bar").style.transform = `scaleX(${shown})`;
  if (outcome !== undefined) {
    if (outcome) return fail(outcome);
    if (shown > 0.999 && now - startedAt > 1800) return complete();
  }
  frame = requestAnimationFrame(tick);
}

function showNotes(id) {
  const note = $(id);
  note.textContent = notes.join(" ");
  note.hidden = notes.length === 0;
}

function complete() {
  ring.set(1);
  ring.done();
  burst();
  $("#progress-file").textContent = "";
  setTimeout(() => {
    setBusy(false);
    showNotes(installing ? "#done-note" : "#removed-note");
    if (installing) {
      $("#done-title").textContent = updating ? "KiwiConvert is up to date" : "KiwiConvert is ready";
      $("#done-lead").textContent = state.desktopShortcut
        ? "Find it on your desktop, in the Start menu, and in the tray next to the clock."
        : "Find it in the Start menu and in the tray next to the clock.";
      show("done");
    } else {
      $("#removed-lead").textContent = state.removeData
        ? "Your settings and history were removed too. Thanks for giving it a try."
        : "Your settings were kept in case you come back. Thanks for giving it a try.";
      show("removed");
    }
  }, 1100);
}

function fail(message) {
  setBusy(false);
  $("#error-title").textContent = installing ? "Setup couldn't finish" : "KiwiConvert couldn't be removed";
  $("#error-text").textContent = message;
  show("error");
}

window.kiwi = {
  progress(fraction, file) {
    target = Math.max(target, Math.min(1, fraction));
    if (file) $("#progress-file").textContent = file;
  },
  finished(error, skipped) {
    outcome = error || null;
    notes = skipped || [];
    if (!error) target = 1;
  },
  folderPicked(path) {
    state.dir = path;
    renderDir();
  },
};

// ---- Wiring ------------------------------------------------------------------------

function renderDir() {
  $("#dir-text").textContent = state.dir;
  $("#dir-text").title = state.dir;
}

function toggle(button) {
  const key = button.dataset.toggle;
  state[key] = !state[key];
  button.setAttribute("aria-checked", String(state[key]));
}

const actions = {
  minimize: () => post("minimize"),
  close: () => !state.busy && post("close"),
  openOptions: () => openSheet("options"),
  openLicense: () => openSheet("license"),
  closeSheet,
  launch: () => post("launch"),
  retry: () => show(installing ? "welcome" : "remove"),
};

document.addEventListener("click", (e) => {
  const link = e.target.closest("a[href]");
  if (link) {
    e.preventDefault();
    post("openUrl", { url: link.href });
    return;
  }
  const button = e.target.closest("[data-action], [data-toggle]");
  if (!button) return;
  if (button.dataset.toggle) toggle(button);
  else actions[button.dataset.action]?.();
});

// Dragging anywhere that isn't a control moves the window.
document.addEventListener("mousedown", (e) => {
  if (e.button !== 0 || e.target.closest("button, a, .sheet, .backdrop, .errbox, .license-text")) return;
  post("drag");
});

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    if (sheet) closeSheet();
    else actions.close();
  }
});

$("#install").addEventListener("click", () => {
  closeSheet();
  startProgress();
  post("install", { dir: state.dir, desktopShortcut: state.desktopShortcut, startWithWindows: state.startWithWindows });
});

$("#uninstall").addEventListener("click", () => {
  startProgress();
  post("uninstall", { removeData: state.removeData });
});

$("#change-dir").addEventListener("click", () => post("pickFolder", { dir: state.dir }));

// ---- Start -------------------------------------------------------------------------

function init() {
  if (updating) {
    const same = K.existing.version === K.version;
    $("#welcome-pill").textContent = same ? `Version ${K.version} is installed` : `Update ${K.existing.version} → ${K.version}`;
    $("#welcome-title").textContent = same ? "Reinstall KiwiConvert" : "Update KiwiConvert";
    $("#install-label").textContent = same ? "Reinstall" : "Update";
    // Updates go where the app already is.
    $("#change-dir").hidden = true;
  } else {
    $("#welcome-pill").textContent = `Version ${K.version} · Free and open source`;
  }
  $("#remove-pill").textContent = `Version ${K.version}`;
  $("#fine-print").textContent = `Just for you · No admin rights needed · ${K.sizeMb} MB`;
  renderDir();

  const spores = $(".spores");
  for (let i = 0; i < 12; i++) {
    const s = document.createElement("i");
    s.style.setProperty("--x", `${Math.random() * 100}%`);
    s.style.setProperty("--s", `${2 + Math.random() * 4}px`);
    s.style.setProperty("--d", `${9 + Math.random() * 9}s`);
    s.style.setProperty("--delay", `${-Math.random() * 18}s`);
    s.style.setProperty("--dx", `${(Math.random() - 0.5) * 60}px`);
    spores.appendChild(s);
  }

  show(installing ? "welcome" : "remove");

  // Show the window once the artwork has painted, so it never opens half drawn.
  const art = $(".hero-art");
  Promise.race([art.decode().catch(() => {}), new Promise((r) => setTimeout(r, 1500))]).then(() =>
    requestAnimationFrame(() => requestAnimationFrame(() => post("ready"))),
  );
}

init();
