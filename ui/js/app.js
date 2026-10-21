import { STRINGS, LANGUAGES } from "./i18n.js";
import { cat, keycaps, keyLabels } from "./cat.js";
import { tauri } from "./bridge.js";
import { initRecordings } from "./recordings.js";
import { initAskSettings } from "./asksettings.js";
import { initToday } from "./today.js";
import { initAssistantSettings } from "./assistantsettings.js";
import { initConnections } from "./connections.js";
import { initLook } from "./look.js";
import { initHome } from "./home.js";

const { invoke, listen, openUrl } = tauri;
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];

let settings = null;
let history = { items: [], total_words: 0, total_seconds: 0 };
let paused = false;
let T = STRINGS.cs;

const ICON = {
  copy: '<svg viewBox="0 0 24 24"><rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h10"/></svg>',
  raw: '<svg viewBox="0 0 24 24"><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/></svg>',
  del: '<svg viewBox="0 0 24 24"><path d="M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3"/></svg>',
  x: '<svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>',
};

// ───────── helpers ─────────
function toast(text) {
  const el = $("#toast");
  el.textContent = text;
  el.classList.add("show");
  clearTimeout(toast.t);
  toast.t = setTimeout(() => el.classList.remove("show"), 1600);
}

function esc(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}

function fmtNum(n) {
  return new Intl.NumberFormat(settings?.ui_lang === "en" ? "en" : "cs").format(Math.round(n));
}

function applyI18n() {
  T = STRINGS[settings?.ui_lang] || STRINGS.cs;
  document.documentElement.lang = settings?.ui_lang || "cs";
  $$("[data-i18n]").forEach((el) => (el.textContent = T[el.dataset.i18n] ?? el.dataset.i18n));
  $$("[data-i18n-html]").forEach((el) => (el.innerHTML = T[el.dataset.i18nHtml] ?? ""));
  $$("[data-i18n-placeholder]").forEach((el) => (el.placeholder = T[el.dataset.i18nPlaceholder] ?? ""));
  $("#ob-key").placeholder = T.key_placeholder;
  $("#set-key").placeholder = T.key_placeholder;
  renderStatus();
  renderLanguages();
}

// Never write settings we didn't actually load (that would wipe the Groq key etc.).
let settingsLoaded = false;
async function save(patch, quiet = false) {
  if (!settingsLoaded) {
    toast(T.not_loaded || "Nastavení se ještě nenačetlo — zkus to za chvilku");
    return false;
  }
  const next = { ...settings, ...patch };
  try {
    settings = await invoke("save_settings", { settings: next });
    if (!quiet) toast(T.saved);
    return true;
  } catch (e) {
    toast(String(e));
    return false;
  }
}

// ───────── navigation ─────────
function go(page) {
  // "settings:connections" opens a settings tab
  if (page && page.startsWith("settings:")) {
    const tab = page.split(":")[1];
    page = "settings";
    setTimeout(() => setTab(tab));
  }
  if (page === "chat") {
    const f = $("#chat-frame");
    if (!f.getAttribute("src")) f.setAttribute("src", "ask.html?embed=1");
    setTimeout(() => { try { f.contentWindow?.focus(); } catch {} }, 200);
  }
  document.body.classList.toggle("on-chat", page === "chat");
  $("#nav-chat").classList.toggle("active", page === "chat");
  $$(".nav-btn[data-page]").forEach((b) => b.classList.toggle("active", b.dataset.page === page));
  $$(".page").forEach((p) => p.classList.toggle("active", p.id === `page-${page}`));
  $(".main").scrollTop = 0;
}
$$(".nav-btn[data-page]").forEach((b) => b.addEventListener("click", () => go(b.dataset.page)));
$("#nav-chat").addEventListener("click", () => go("chat"));
// the embedded chat asks us to switch page (its Dashboard / Settings / close)
window.addEventListener("message", (e) => {
  const m = e.data;
  if (!m || !m.whiskr) return;
  if (m.goto) go(m.goto);
  if (m.chats !== undefined) {
    $("#cs-list").innerHTML = m.chats;
    if (m.ph) $("#cs-search").placeholder = m.ph;
    if (m.nl) $("#cs-new-l").textContent = m.nl;
  }
});
const chatCmd = (c) => $("#chat-frame").contentWindow?.postMessage({ whiskr: true, chatCmd: c }, "*");
$("#chat-back").addEventListener("click", () => go("home"));
$("#cs-new").addEventListener("click", () => chatCmd({ type: "new" }));
$("#cs-search").addEventListener("input", (e) => chatCmd({ type: "search", q: e.target.value }));
$("#cs-list").addEventListener("click", (e) => {
  const del = e.target.closest("[data-del]");
  if (del) return chatCmd({ type: "del", id: del.dataset.del });
  const it = e.target.closest("[data-conv]");
  if (it) {
    $$("#cs-list .side-item").forEach((x) => x.classList.toggle("on", x === it));
    chatCmd({ type: "open", id: it.dataset.conv });
  }
});

// collapsible sidebar (folded by default, like the side panel in the chat)
function sideOpen() {
  try {
    return localStorage.getItem("whiskr-main-side") === "1";
  } catch {
    return false;
  }
}
function setSide(open) {
  document.body.classList.toggle("side-open", open);
  try {
    localStorage.setItem("whiskr-main-side", open ? "1" : "0");
  } catch {}
}
setSide(false); // always starts folded; Ctrl+B / the icon opens it
$("#side-open").addEventListener("click", () => setSide(true));
$("#side-toggle").addEventListener("click", () => setSide(!document.body.classList.contains("side-open")));
document.addEventListener("keydown", (e) => {
  if (e.ctrlKey && (e.key === "b" || e.key === "B")) {
    e.preventDefault();
    setSide(!document.body.classList.contains("side-open"));
  }
});

// settings tabs
function setTab(tab) {
  if (!$(`.set-tab[data-tab="${tab}"]`)) tab = "general";
  $$("#set-tabs button").forEach((b) => b.classList.toggle("on", b.dataset.tab === tab));
  $$(".set-tab").forEach((p) => p.classList.toggle("on", p.dataset.tab === tab));
  try {
    localStorage.setItem("whiskr-set-tab", tab);
  } catch {}
  if (tab === "connections") conns?.open();
}
$("#set-tabs").addEventListener("click", (e) => {
  const b = e.target.closest("[data-tab]");
  if (b) setTab(b.dataset.tab);
});
function storedTab() {
  try {
    return localStorage.getItem("whiskr-set-tab") || "general";
  } catch {
    return "general";
  }
}
$$("[data-goto]").forEach((b) => b.addEventListener("click", () => go(b.dataset.goto)));

// ───────── status / pause ─────────
// ───────── updates ─────────
let upd = null;
const fmtV = (t, v) => String(t).replace("{v}", v || "");
function renderUpdate() {
  const box = $("#upd");
  if (!box) return;
  box.hidden = !(upd && upd.available);
  if (upd?.available) {
    $("#upd-t").textContent = fmtV(T.upd_have, upd.latest);
    $("#upd-go").textContent = T.upd_go;
  }
}
async function installUpdate(btn) {
  if (btn) btn.disabled = true;
  $("#upd-bar").hidden = false;
  $("#upd-t").textContent = T.upd_dl;
  try {
    await invoke("install_update");
    $("#upd-t").textContent = T.upd_inst;
  } catch (e) {
    toast(String(e));
    $("#upd-bar").hidden = true;
    if (btn) btn.disabled = false;
    renderUpdate();
  }
}
$("#upd-go")?.addEventListener("click", (e) => installUpdate(e.currentTarget));
listen("whiskr:update", (ev) => {
  upd = ev.payload;
  renderUpdate();
});
listen("whiskr:update-progress", (ev) => {
  $("#upd").hidden = false;
  $("#upd-bar").hidden = false;
  $("#upd-fill").style.width = Math.round((ev.payload || 0) * 100) + "%";
  if (ev.payload >= 1) $("#upd-t").textContent = T.upd_inst;
});
$("#upd-check")?.addEventListener("click", async (e) => {
  const b = e.currentTarget;
  b.disabled = true;
  $("#upd-d").textContent = T.upd_checking;
  try {
    upd = await invoke("check_update");
    if (upd?.available) {
      $("#upd-d").textContent = fmtV(T.upd_have, upd.latest);
      b.textContent = T.upd_go;
      b.onclick = () => installUpdate(b);
    } else {
      $("#upd-d").textContent = fmtV(T.upd_latest, upd?.current);
    }
    renderUpdate();
  } catch (err) {
    $("#upd-d").textContent = String(err);
  }
  b.disabled = false;
});

let lock = { locked: false, game: false };
function renderStatus() {
  $("#status-card").classList.toggle("paused", paused);
  $("#status-card").classList.toggle("locked", lock.locked || lock.game);
  $("#status-text").textContent = paused ? T.status_paused : lock.locked ? T.status_locked : lock.game ? T.status_game : T.status_on;
  $("#pause-btn").textContent = paused ? T.resume : T.pause;
  $("#lock-btn").classList.toggle("on", lock.locked);
  $("#lock-btn").title = `${lock.locked ? T.lock_off : T.lock_on} · Ctrl+Alt+Shift+L`;
  if ($("#lock-btn2")) $("#lock-btn2").textContent = lock.locked ? T.lock_off : T.lock_on;
}
async function toggleLock() {
  lock = (await invoke("toggle_lock").catch(() => null)) || { ...lock, locked: !lock.locked };
  renderStatus();
}
$("#lock-btn").addEventListener("click", toggleLock);
$("#lock-btn2")?.addEventListener("click", toggleLock);
listen("whiskr:lock", (ev) => {
  lock = ev.payload || lock;
  renderStatus();
});
$("#games")?.addEventListener("change", (e) => {
  const list = e.target.value.split(/\n/).map((x) => x.trim().toLowerCase()).filter(Boolean);
  save({ game_apps: list }, true);
});
$("#pause-btn").addEventListener("click", async () => {
  paused = await invoke("toggle_paused");
  renderStatus();
});

// ───────── home ─────────
const HK_FIELDS = ["hotkey_ask", "hotkey_task", "hotkey", "hotkey_command", "hotkey_record", "hotkey_paste", "hotkey_bookmark"];

function renderHotkeys() {
  const caps = settings.hotkey ? keycaps(settings.hotkey) : `<span class="muted">${T.hk_disabled}</span>`;
  $("#home-keys").innerHTML = caps;
  $("#ob-keys").innerHTML = caps;
  $$(".hk").forEach((row) => {
    const spec = settings[row.dataset.field] || "";
    $(".hk-keys", row).innerHTML = spec ? keycaps(spec) : `<span class="muted small">${T.hk_disabled}</span>`;
    $(".hk-off", row).hidden = !spec;
  });
  const tips = [
    ["hotkey_ask", T.tip2_ask],
    ["hotkey_task", T.tip2_task],
    ["hotkey_command", T.tip2_command],
    ["hotkey_record", T.tip2_record],
    ["hotkey_paste", T.tip2_paste],
    ["hotkey_bookmark", T.tip2_bookmark],
  ]
    .filter(([f]) => settings[f])
    .map(([f, name]) => `<span>${esc(keyLabels(settings[f]).join("+"))} = ${esc(name)}</span>`)
    .join("<span>·</span>");
  $("#home-tips2").innerHTML = tips;
}

function renderStats() {
  const words = history.total_words || 0;
  const secs = history.total_seconds || 0;
  // Typing ~40 wpm vs. the time actually spent talking.
  const savedMin = Math.max(0, words / 40 - secs / 60);
  $("#stat-words").textContent = fmtNum(words);
  $("#stat-minutes").textContent = fmtNum(secs / 60);
  $("#stat-saved").textContent = savedMin >= 60 ? `${(savedMin / 60).toFixed(1)} h` : `${fmtNum(savedMin)} min`;
}

function entryHtml(e) {
  const time = new Date(e.at).toLocaleTimeString(settings.ui_lang === "en" ? "en" : "cs", { hour: "2-digit", minute: "2-digit" });
  const showRaw = e.raw && e.raw.trim() !== e.text.trim();
  return `<div class="entry" data-id="${e.id}">
    <time>${time}</time>
    <div style="min-width:0">
      <div class="text">${esc(e.text)}</div>
      ${showRaw ? `<div class="raw"><b>${T.raw_label}</b><br>${esc(e.raw)}</div>` : ""}
    </div>
    <div class="actions">
      <button class="icon-btn" data-act="copy" title="${T.copy}">${ICON.copy}</button>
      ${showRaw ? `<button class="icon-btn" data-act="raw" title="${T.raw}">${ICON.raw}</button>` : ""}
      <button class="icon-btn" data-act="del" title="${T.delete}">${ICON.del}</button>
    </div>
  </div>`;
}

function emptyHtml(text) {
  return `<div class="empty">${cat({ eyes: "var(--paper)" })}${text}</div>`;
}

function renderRecent() {
  const items = history.items.slice(0, 4);
  $("#recent").innerHTML = items.length ? items.map(entryHtml).join("") : emptyHtml(T.empty_recent);
}

function dayLabel(d) {
  const today = new Date(); today.setHours(0, 0, 0, 0);
  const day = new Date(d); day.setHours(0, 0, 0, 0);
  const diff = (today - day) / 86400000;
  if (diff === 0) return T.today;
  if (diff === 1) return T.yesterday;
  return day.toLocaleDateString(settings.ui_lang === "en" ? "en" : "cs", { weekday: "long", day: "numeric", month: "long" });
}

function renderHistory() {
  const q = $("#hist-search").value.trim().toLowerCase();
  const items = history.items.filter((e) => !q || e.text.toLowerCase().includes(q) || (e.raw || "").toLowerCase().includes(q));
  if (!items.length) {
    $("#history-list").innerHTML = emptyHtml(T.hist_empty);
    return;
  }
  let html = "", current = "";
  for (const e of items) {
    const label = dayLabel(e.at);
    if (label !== current) {
      if (current) html += "</div>";
      html += `<div class="day-label">${label}</div><div class="card" style="padding:6px 18px">`;
      current = label;
    }
    html += entryHtml(e);
  }
  $("#history-list").innerHTML = html + "</div>";
}

function renderAll() {
  renderStats();
  renderRecent();
  renderHistory();
}

document.addEventListener("click", async (ev) => {
  const btn = ev.target.closest(".entry [data-act]");
  if (!btn) return;
  const row = btn.closest(".entry");
  const id = Number(row.dataset.id);
  const entry = history.items.find((e) => e.id === id);
  if (!entry) return;
  if (btn.dataset.act === "copy") {
    await invoke("copy_text", { text: entry.text });
    toast(T.copied);
  } else if (btn.dataset.act === "raw") {
    $$(`.entry[data-id="${id}"]`).forEach((r) => r.classList.toggle("show-raw"));
  } else if (btn.dataset.act === "del") {
    await invoke("delete_entry", { id });
    history.items = history.items.filter((e) => e.id !== id);
    renderAll();
  }
});

$("#hist-search").addEventListener("input", renderHistory);
$("#clear-history").addEventListener("click", async () => {
  if (!history.items.length || !confirm(T.hist_clear_confirm)) return;
  await invoke("clear_history");
  history.items = [];
  renderAll();
});

// ───────── onboarding & key ─────────
const GROQ_KEYS = "https://console.groq.com/keys";
$("#open-groq").addEventListener("click", () => openUrl(GROQ_KEYS));
$("#get-key").addEventListener("click", () => openUrl(GROQ_KEYS));

async function verifyKey(input, button, msg, onOk) {
  const key = input.value.trim();
  if (!key) { input.focus(); return; }
  button.disabled = true;
  button.textContent = T.verifying;
  msg.className = "msg";
  msg.textContent = "";
  try {
    await invoke("check_key", { key });
    await save({ api_key: key }, true);
    msg.className = "msg ok";
    msg.textContent = T.key_ok;
    onOk?.();
  } catch (e) {
    msg.className = "msg err";
    msg.textContent = String(e);
  } finally {
    button.disabled = false;
    button.textContent = T.verify;
  }
}

$("#ob-verify").addEventListener("click", () =>
  verifyKey($("#ob-key"), $("#ob-verify"), $("#ob-msg"), () => {
    $("#step-2").classList.add("done");
    $("#step-1").classList.add("done");
    $("#step-3").classList.remove("locked");
    $("#step-3 textarea").focus();
    $("#set-key").value = settings.api_key;
  })
);
$("#ob-key").addEventListener("keydown", (e) => e.key === "Enter" && $("#ob-verify").click());
$("#set-verify").addEventListener("click", () => verifyKey($("#set-key"), $("#set-verify"), $("#key-msg")));
$("#set-key").addEventListener("keydown", (e) => e.key === "Enter" && $("#set-verify").click());

function renderOnboarding() {
  const needsKey = !settings.api_key;
  // Once shown, keep the onboarding visible until the user leaves Home,
  // so step 3 (try it) doesn't vanish the moment the key is saved.
  if (needsKey) renderOnboarding.active = true;
  const show = needsKey || renderOnboarding.active;
  $("#onboarding").hidden = !show;
  $("#home").hidden = show;
}
$$(".nav-btn").forEach((b) => b.addEventListener("click", () => {
  if (settings.api_key && renderOnboarding.active) {
    renderOnboarding.active = false;
    renderOnboarding();
  }
}));

// ───────── dictionary ─────────
function renderDictionary() {
  const words = settings.dictionary || [];
  $("#dict-chips").innerHTML = words.length
    ? words.map((w, i) => `<span class="chip">${esc(w)}<button data-i="${i}" title="${T.delete}">${ICON.x}</button></span>`).join("")
    : `<span class="muted small">${T.dict_empty}</span>`;
}
async function addWords() {
  const input = $("#dict-input");
  const words = input.value.split(",").map((w) => w.trim()).filter(Boolean);
  if (!words.length) return;
  const set = new Set(settings.dictionary || []);
  words.forEach((w) => set.add(w));
  if (await save({ dictionary: [...set] }, true)) {
    input.value = "";
    renderDictionary();
  }
}
$("#dict-add").addEventListener("click", addWords);
$("#dict-input").addEventListener("keydown", (e) => e.key === "Enter" && addWords());
$("#dict-chips").addEventListener("click", async (e) => {
  const b = e.target.closest("button[data-i]");
  if (!b) return;
  const dictionary = settings.dictionary.filter((_, i) => i !== Number(b.dataset.i));
  if (await save({ dictionary }, true)) renderDictionary();
});

// ───────── settings ─────────
function renderLanguages() {
  if (!settings) return;
  $("#set-language").innerHTML = LANGUAGES.map(
    ([code, name]) => `<option value="${code}">${name ?? T.lang_auto}</option>`
  ).join("");
  $("#set-language").value = settings.language;
}

// ───────── theme & window frame ─────────
const darkMq = window.matchMedia("(prefers-color-scheme: dark)");
function applyTheme() {
  const t = settings?.theme || "dark";
  const dark = t === "dark" || (t === "system" && darkMq.matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
  lookUi?.apply();
  invoke("set_window_theme", { dark }).catch(() => {});
  $$("#set-theme .seg").forEach((b) => b.classList.toggle("on", b.dataset.v === t));
}
darkMq.addEventListener?.("change", applyTheme);
$$("#set-theme .seg").forEach((b) =>
  b.addEventListener("click", async () => {
    if (await save({ theme: b.dataset.v }, true)) applyTheme();
  })
);
const appWin = window.__TAURI__?.window?.getCurrentWindow?.();
$("#win-min").addEventListener("click", () => appWin?.minimize());
$("#win-max").addEventListener("click", () => appWin?.toggleMaximize());
$("#win-close").addEventListener("click", () => appWin?.close());
$(".dragbar").addEventListener("dblclick", () => appWin?.toggleMaximize());
// rounded corners only when not maximized
async function syncMaxed() {
  try {
    document.body.classList.toggle("maxed", !!(await appWin?.isMaximized()));
  } catch {}
}
appWin?.onResized?.(() => syncMaxed());
syncMaxed();

function renderSettings() {
  renderLanguages();
  applyTheme();
  $$("#set-polish .seg").forEach((s) => s.classList.toggle("on", s.dataset.v === settings.polish));
  $$("#set-bar .seg").forEach((s) => s.classList.toggle("on", s.dataset.v === (settings.bar_style || "classic")));
  $("#set-sounds").checked = settings.sounds;
  $("#set-splash").checked = settings.splash !== false;
  if ($("#set-debug")) $("#set-debug").checked = !!settings.debug_log;
  $("#set-tone").checked = settings.app_tone !== false;
  $("#set-autostart").checked = settings.autostart;
  $("#set-ui").value = settings.ui_lang;
  $("#set-key").value = settings.api_key;
}

async function renderMics() {
  const mics = await invoke("list_mics").catch(() => []);
  $("#set-mic").innerHTML =
    `<option value="">${T.mic_default}</option>` +
    mics.map((m) => `<option value="${esc(m)}">${esc(m)}</option>`).join("");
  $("#set-mic").value = settings.microphone || "";
}

$("#set-splash").addEventListener("change", (e) => save({ splash: e.target.checked }));
$("#set-debug")?.addEventListener("change", (e) => save({ debug_log: e.target.checked }));
$("#set-language").addEventListener("change", (e) => save({ language: e.target.value }));
$("#set-mic").addEventListener("change", (e) => save({ microphone: e.target.value || null }));
$("#set-sounds").addEventListener("change", (e) => save({ sounds: e.target.checked }));
$("#set-autostart").addEventListener("change", async (e) => {
  if (!(await save({ autostart: e.target.checked }))) e.target.checked = !e.target.checked;
});
$("#set-ui").addEventListener("change", async (e) => {
  await save({ ui_lang: e.target.value }, true);
  applyI18n();
  renderAll();
  renderMics();
  recs?.rerender();
  askSet?.render();
  today?.render();
  assistSet?.render();
  conns?.render();
  lookUi?.render();
  home?.render();
  renderDictionary();
});
$$("#set-polish .seg").forEach((s) =>
  s.addEventListener("click", async () => {
    if (await save({ polish: s.dataset.v })) renderSettings();
  })
);

$$("#set-bar .seg").forEach((s) =>
  s.addEventListener("click", async () => {
    if (await save({ bar_style: s.dataset.v }, true)) {
      renderSettings();
      invoke("preview_bar").catch(() => {});
    }
  })
);

// Hotkey capture
const MOD_CODES = new Set(["ControlLeft", "ControlRight", "ShiftLeft", "ShiftRight", "AltLeft", "AltRight", "MetaLeft", "MetaRight", "OSLeft", "OSRight"]);
let capturing = null; // settings field being captured

async function startCapture(field) {
  if (capturing) await endCapture();
  capturing = field;
  await invoke("suspend_hotkey", { suspend: true }).catch(() => {});
  const row = $(`.hk[data-field="${field}"]`);
  $(".hk-keys", row).innerHTML = `<span class="capture">${T.set_hotkey_press}</span>`;
  $("#hotkey-msg").textContent = "";
  window.addEventListener("keydown", onCaptureKey, true);
}

async function endCapture() {
  capturing = null;
  window.removeEventListener("keydown", onCaptureKey, true);
  await invoke("suspend_hotkey", { suspend: false }).catch((e) => ($("#hotkey-msg").textContent = String(e)));
  renderHotkeys();
}

async function onCaptureKey(e) {
  e.preventDefault();
  e.stopPropagation();
  if (e.code === "Escape") return endCapture();
  if (MOD_CODES.has(e.code)) return; // wait for the main key
  const mods = [];
  if (e.ctrlKey) mods.push("ctrl");
  if (e.altKey) mods.push("alt");
  if (e.shiftKey) mods.push("shift");
  if (e.metaKey) mods.push("super");
  const isF = /^F\d{1,2}$/.test(e.code);
  if (!mods.length && !isF) {
    $("#hotkey-msg").textContent = T.set_hotkey_need_mod;
    return;
  }
  if (e.ctrlKey && e.altKey) {
    $("#hotkey-msg").textContent = T.set_hotkey_altgr;
    return;
  }
  const spec = [...mods, e.code].join("+");
  const field = capturing;
  if (HK_FIELDS.some((f) => f !== field && (settings[f] || "").toLowerCase() === spec.toLowerCase())) {
    $("#hotkey-msg").textContent = T.hk_dupe;
    return;
  }
  window.removeEventListener("keydown", onCaptureKey, true);
  capturing = null;
  const ok = await save({ [field]: spec });
  if (ok) $("#hotkey-msg").textContent = "";
  await invoke("suspend_hotkey", { suspend: false }).catch(() => {});
  renderHotkeys();
}

$$(".hk").forEach((row) => {
  $(".hk-change", row).addEventListener("click", () => startCapture(row.dataset.field));
  $(".hk-off", row).addEventListener("click", async () => {
    if (await save({ [row.dataset.field]: "" })) renderHotkeys();
  });
});

$("#set-tone").addEventListener("change", (e) => save({ app_tone: e.target.checked }));

// ───────── live events ─────────
listen("whiskr:history", (ev) => {
  history.items.unshift(ev.payload);
  history.total_words += ev.payload.words;
  history.total_seconds += ev.payload.seconds;
  renderAll();
});
listen("whiskr:paused", (ev) => {
  paused = ev.payload;
  renderStatus();
});
listen("whiskr:state", (ev) => {
  const p = ev.payload.phase;
  const live = $("#live");
  const on = p === "listening" || p === "handsfree" || p === "thinking";
  live.classList.toggle("on", on);
  $("#live-text").textContent = p === "thinking" ? "…" : p === "handsfree" ? T.tip_tap : T.hold_to_talk;
  if (p === "error" && ev.payload.message) toast(ev.payload.message);
});

// ───────── boot ─────────
const DEFAULTS = {
  api_key: "", hotkey: "ctrl+shift+Space", hotkey_command: "alt+shift+KeyE", hotkey_record: "alt+shift+KeyR", hotkey_paste: "alt+shift+KeyV", hotkey_ask: "shift+Space", hotkey_task: "alt+shift+KeyT", theme: "dark", hotkey_bookmark: "alt+shift+KeyB", brain: "auto", briefing: true, briefing_time: "08:00", briefing_open: true, briefing_all_tools: true, briefing_tools: [], briefing_extra: "", meeting_detect: true, meeting_autostop: true, default_template: "auto", custom_templates: [], app_tone: true, language: "cs", polish: "light",
  dictionary: [], sounds: true, autostart: false, microphone: null, ui_lang: "cs", bar_style: "classic",
};

async function safe(cmd, args, fallback) {
  try {
    return await invoke(cmd, args);
  } catch (e) {
    console.error(cmd, e);
    safe.errors.push(`${cmd}: ${e}`);
    return fallback;
  }
}
safe.errors = [];

window.addEventListener("error", (e) => toast(String(e.message || e)));
window.addEventListener("unhandledrejection", (e) => toast(String(e.reason || e)));

let recs = null;
let askSet = null;
let today = null;
let assistSet = null;
let conns = null;
let lookUi = null;
let home = null;

(async function boot() {
  $("#brand-cat").innerHTML = cat({ body: "var(--black)" });
  $$(".bp .c").forEach((el) => (el.innerHTML = cat({ body: "var(--cream)", eyes: "var(--black)" })));
  // Texts first, so the UI is never empty even if the core is slow or fails.
  settings = { ...DEFAULTS };
  applyI18n();

  // The core may still be starting when this page loads: wait for it.
  let loaded = null;
  for (let i = 0; i < 60 && !loaded; i++) {
    try {
      loaded = await invoke("get_settings");
    } catch (e) {
      await new Promise((r) => setTimeout(r, 150));
    }
  }
  settingsLoaded = !!loaded;
  if (!loaded) safe.errors.push("get_settings: Whiskr core did not answer");
  settings = { ...DEFAULTS, ...(loaded || {}) };
  history = await safe("get_history", {}, history);
  paused = await safe("get_paused", {}, false);
  lock = (await safe("get_lock", {}, lock)) || lock;
  if ($("#games")) $("#games").value = (settings.game_apps || []).join("\n");
  const ver = await safe("app_version", {}, "");
  $("#version").textContent = "Whiskr " + ver;
  if ($("#upd-d")) $("#upd-d").textContent = fmtV(T.upd_cur, ver);

  recs = initRecordings({
    invoke, listen, openUrl, toast, esc, $, $$, go,
    T: () => T,
    settings: () => settings,
    save: async (patch, quiet) => save(patch, quiet),
  });
  askSet = initAskSettings({ invoke, listen, toast, esc, $, $$, T: () => T, settings: () => settings, save: async (patch, quiet) => save(patch, quiet) });
  today = initToday({ invoke, listen, toast, esc, $, $$, T: () => T, settings: () => settings });
  assistSet = initAssistantSettings({ invoke, toast, esc, $, $$, T: () => T, settings: () => settings, save: async (patch, quiet) => save(patch, quiet) });
  home = initHome({ invoke, listen, esc, $, T: () => T, settings: () => settings, go, toast, save: async (patch, quiet) => save(patch, quiet) });
  lookUi = initLook({ invoke, toast, esc, $, $$, T: () => T, settings: () => settings, save: async (patch, quiet) => save(patch, quiet) });
  conns = initConnections({ invoke, toast, esc, $, T: () => T, settings: () => settings, save: async (patch, quiet) => save(patch, quiet) });
  const steps = [() => conns.init(), applyI18n, renderHotkeys, renderOnboarding, renderSettings, renderDictionary, renderAll, renderMics, () => recs.init(), () => askSet.init(), () => today.init(), () => assistSet.init(), () => lookUi.init(), () => home.init(), () => setTab(storedTab())];
  for (const step of steps) {
    try { await step(); } catch (e) { console.error(e); safe.errors.push(String(e)); }
  }
  if (safe.errors.length) toast("⚠ " + safe.errors[0]);
})();
