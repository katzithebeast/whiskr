// Whiskr Ask — one shortcut for quick questions, a Claude × GPT debate,
// and delegating work to Claude Code / Codex / Cursor in a project folder.
import { cat } from "./cat.js";

const TAURI = window.__TAURI__;
const api = TAURI ? { invoke: TAURI.core.invoke, listen: TAURI.event.listen } : mockApi();
// Embedded = shown inside the main window (Chat page) instead of the popup.
const EMBED = new URLSearchParams(location.search).has("embed");
if (EMBED) document.documentElement.classList.add("embed");
const toParent = (msg) => window.parent.postMessage({ whiskr: true, ...msg }, "*");
const POPUP_ONLY = ["ask_layout", "ask_region", "set_glass", "ask_hide", "ask_close_cancel", "ask_hide_after"];
const listen = api.listen;
const dbg = (m) => { try { api.invoke("dbg_log", { msg: m }).catch(() => {}); } catch {} };
const invoke = (cmd, args) => {
  if (EMBED && POPUP_ONLY.includes(cmd)) return Promise.resolve(null);
  if (EMBED && cmd === "open_main") { toParent({ goto: args?.page || "home" }); return Promise.resolve(null); }
  return api.invoke(cmd, args);
};
const $ = (s, el = document) => el.querySelector(s);

const ENGINES = {
  claude: { name: "Claude", color: "var(--claude)" },
  codex: { name: "GPT", sub: "Codex", color: "var(--codex)" },
  cursor: { name: "Cursor", color: "var(--cursor)" },
  groq: { name: "Groq", sub: "rychlý, zdarma", color: "var(--groq)" },
  whiskr: { name: "Whiskr", color: "var(--cream)" },
};

const L = {
  cs: {
    m_ask: "Zeptat se", m_debate: "Debata", m_delegate: "Delegovat",
    v_listen: "Poslouchám… pusť Shift+Backspace", v_think: "Přepisuju…", v_empty: "Nic jsem neslyšela 🙉",
    ph_ask: "Na co se chceš zeptat?", ph_debate: "Otázka pro Claude i GPT najednou…", ph_delegate: "Co má agent v projektu udělat?",
    send: "Odeslat", stop: "Zastavit", insert: "Vložit", copy: "Kopírovat", copied: "Zkopírováno ✓",
    open_chatgpt: "Pokračovat v ChatGPT", open_claude: "Pokračovat v Claude",
    debate_more: "⚔ Ať to proberou", debate_sum: "✦ Shrň debatu", round: "Kolo", synth: "Shrnutí debaty",
    sel: "Označený text", from: "z", project: "Projekt", add_project: "＋ Přidat složku…", no_project: "Vyber projekt…",
    safe: "🛡 Bezpečně", full: "⚡ Plný přístup",
    safe_t: "Agent smí upravovat soubory v projektu, ale nespouští libovolné příkazy.",
    full_t: "Agent smí všechno včetně příkazů. Používej jen u projektů, kterým věříš.",
    thinking: "Přemýšlí…", working: "Pracuje…", steps_more: "zobrazit všechny kroky", steps_less: "skrýt kroky",
    h_send: "<b>Enter</b> odeslat", h_nl: "<b>Shift+Enter</b> nový řádek", h_modes: "<b>Ctrl+1/2/3</b> režim", h_at: "<b>@gpt</b> <b>@claude</b> model", h_esc: "<b>Esc</b> zavřít", h_ins: "<b>Ctrl+Enter</b> vložit odpověď",
    not_installed: "nenainstalováno", no_key: "chybí klíč", runs: "běží",
    new: "Nový chat", hist: "Historie", pin: "Nechat otevřené", hist_empty: "Zatím žádné konverzace.",
    empty_ask: "Zeptej se na cokoliv. <b>Označený text</b> se přiloží sám.",
    empty_debate: "Claude a GPT odpoví zároveň. Pak je nech <b>probrat</b> a <b>shrnout</b>.",
    empty_delegate: "Zadej úkol, vyber projekt a agent ho udělá. Můžeš zatím dělat něco jiného — dám vědět, až bude hotovo.",
    need_two: "Na debatu potřebuju aspoň dva modely (Claude, Codex, Cursor nebo Groq).",
    profile_t: "Účet: klikni pro přepnutí osobní / práce",
    sum_prompt: (q, a, an, b, bn) => `Původní otázka:\n${q}\n\nOdpověď ${an}:\n${a}\n\nOdpověď ${bn}:\n${b}\n\nUdělej z toho jednu nejlepší odpověď pro uživatele. Na konci přidej krátce „Shoda:“ a „Rozdíly:“. Odpovídej stejným jazykem jako otázka.`,
    debate_prompt: (n, t) => `Na stejnou otázku odpověděl i jiný model (${n}). Jeho odpověď:\n---\n${t}\n---\nZhodnoť ji věcně: s čím souhlasíš, co je špatně nebo chybí a proč. Pak napiš svou vylepšenou odpověď. Buď stručný a odpovídej stejným jazykem jako původní otázka.`,
    sel_only: "Pomoz mi s tímhle textem — vysvětli ho, nebo udělej to, co dává smysl:",
    task_added: "✓ Uloženo do **Dnes**. Čas připomínky si přečtu sama.",
    open_app: "Otevřít Whiskr", dashboard: "Dashboard", settings: "Nastavení", new_chat: "Nový chat", more: "Další možnosti", def_model: "Výchozí model", loading_models: "Načítám modely…", search: "Hledat v chatech…", g_today: "Dnes", g_yesterday: "Včera", g_week: "Tento týden", g_older: "Starší", del: "Smazat",
    err_auth: "Přihlášení vypršelo — přihlas se znovu (otevře se okno terminálu, stačí potvrdit v prohlížeči).", relogin: "Přihlásit znovu",
    h_slash: "<b>/brief</b> Acme · <b>/úkol</b> …",
  },
  en: {
    m_ask: "Ask", m_debate: "Debate", m_delegate: "Delegate",
    v_listen: "Listening… let go of Shift+Backspace", v_think: "Transcribing…", v_empty: "Didn't catch anything 🙉",
    ph_ask: "What do you want to know?", ph_debate: "One question for Claude and GPT…", ph_delegate: "What should the agent do in the project?",
    send: "Send", stop: "Stop", insert: "Insert", copy: "Copy", copied: "Copied ✓",
    open_chatgpt: "Continue in ChatGPT", open_claude: "Continue in Claude",
    debate_more: "⚔ Let them debate", debate_sum: "✦ Sum it up", round: "Round", synth: "Debate summary",
    sel: "Selected text", from: "from", project: "Project", add_project: "＋ Add folder…", no_project: "Pick a project…",
    safe: "🛡 Safe", full: "⚡ Full access",
    safe_t: "The agent may edit files in the project but won't run arbitrary commands.",
    full_t: "The agent may do anything, commands included. Only for projects you trust.",
    thinking: "Thinking…", working: "Working…", steps_more: "show all steps", steps_less: "hide steps",
    h_send: "<b>Enter</b> send", h_nl: "<b>Shift+Enter</b> new line", h_modes: "<b>Ctrl+1/2/3</b> mode", h_at: "<b>@gpt</b> <b>@claude</b> model", h_esc: "<b>Esc</b> close", h_ins: "<b>Ctrl+Enter</b> insert answer",
    not_installed: "not installed", no_key: "no key", runs: "running",
    new: "New chat", hist: "History", pin: "Keep open", hist_empty: "No conversations yet.",
    empty_ask: "Ask anything. <b>Selected text</b> is attached automatically.",
    empty_debate: "Claude and GPT answer side by side. Then let them <b>debate</b> and <b>sum it up</b>.",
    empty_delegate: "Describe a task, pick a project and the agent does it. Keep working — I'll tell you when it's done.",
    need_two: "A debate needs at least two models (Claude, Codex, Cursor or Groq).",
    profile_t: "Account: click to switch personal / work",
    sum_prompt: (q, a, an, b, bn) => `Original question:\n${q}\n\nAnswer from ${an}:\n${a}\n\nAnswer from ${bn}:\n${b}\n\nTurn this into the single best answer for the user. End with a short "Agreement:" and "Differences:". Answer in the question's language.`,
    debate_prompt: (n, t) => `Another model (${n}) answered the same question:\n---\n${t}\n---\nAssess it factually: what you agree with, what's wrong or missing and why. Then give your improved answer. Be concise and answer in the question's language.`,
    sel_only: "Help me with this text — explain it, or do what makes sense:",
    task_added: "✓ Saved to **Today**. I'll read the reminder time myself.",
    open_app: "Open Whiskr", dashboard: "Dashboard", settings: "Settings", new_chat: "New chat", more: "More", def_model: "Default model", loading_models: "Loading models…", search: "Search chats…", g_today: "Today", g_yesterday: "Yesterday", g_week: "This week", g_older: "Older", del: "Delete",
    err_auth: "Your login expired — log in again (a terminal opens, just confirm in the browser).", relogin: "Log in again",
    h_slash: "<b>/brief</b> Acme · <b>/task</b> …",
  },
};
let T = L.cs;

// ───────── state ─────────
let S = {};
let tools = { tools: [], groq: false };
let conv = null;
let convs = [];
let pinned = false;
let selection = null; // { text, app }
let dialogOpen = false;
const runs = new Map(); // run id → answer object
const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));

function newConv() {
  return {
    id: Date.now().toString(36),
    title: "",
    mode: "ask",
    engine: S.ask_engine || "claude",
    cwd: "",
    sessions: {},
    items: [],
    updated: Date.now(),
  };
}

// ───────── models ─────────
const modelCache = {
  // Claude Code aliases always point at the newest model of each family
  claude: [{ id: "fable", name: "Fable 5.1" }, { id: "opus", name: "Opus 5.5" }, { id: "sonnet", name: "Sonnet 5.5" }, { id: "haiku", name: "Haiku 5.5" }],
}; // engine → [{id, name}] | "loading"
function modelFor(engine) {
  return (S.ask_models || {})[engine] || "";
}
function modelName(engine, id = modelFor(engine)) {
  if (!id) return ENGINES[engine]?.name || engine;
  const list = Array.isArray(modelCache[engine]) ? modelCache[engine] : [];
  return list.find((m) => m.id === id)?.name || id;
}
async function loadModels(engine) {
  if (modelCache[engine]) return;
  modelCache[engine] = "loading";
  try {
    modelCache[engine] = await invoke("ask_models", { engine });
  } catch {
    modelCache[engine] = [];
  }
}

function available(engine) {
  if (engine === "groq") return tools.groq;
  return tools.tools.some((t) => t.engine === engine);
}
function toolInfo(engine) {
  const ts = tools.tools.filter((t) => t.engine === engine);
  if (!ts.length) return null;
  return ts.map((t) => (t.runtime === "wsl" ? "WSL" : "Windows")).join(" + ");
}
function firstAvailable(except = []) {
  return ["claude", "codex", "cursor", "groq"].find((e) => !except.includes(e) && available(e));
}

function profiles() {
  return S.profiles?.length ? S.profiles : [{ id: "personal", name: "Osobní", key: "" }];
}
function project() {
  return (S.projects || []).find((p) => p.path === conv.cwd);
}
function effectiveProfile() {
  const p = conv.mode === "delegate" ? project() : null;
  const id = p?.profile || S.active_profile || "personal";
  return profiles().find((x) => x.id === id) || profiles()[0];
}

// ───────── markdown (small & safe) ─────────
function inline(s) {
  return s
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<b>$1</b>")
    .replace(/(^|[^*])\*([^*\n]+)\*/g, "$1<i>$2</i>")
    .replace(/\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g, '<a href="$2" data-url="$2">$1</a>')
    .replace(/(^|[\s(])(https?:\/\/[^\s<)]+)/g, '$1<a href="$2" data-url="$2">$2</a>');
}
function md(src, streaming) {
  const out = [];
  const parts = String(src).split(/```/);
  parts.forEach((part, i) => {
    if (i % 2 === 1) {
      const nl = part.indexOf("\n");
      const code = nl >= 0 ? part.slice(nl + 1) : part;
      out.push(`<pre><button class="cp">${T.copy}</button><code>${esc(code.replace(/\n$/, ""))}</code></pre>`);
      return;
    }
    let list = null;
    const close = () => {
      if (list) out.push(`</${list}>`);
      list = null;
    };
    for (const raw of part.split("\n")) {
      const line = esc(raw);
      let m;
      if ((m = line.match(/^\s*[-*•]\s+(.*)$/))) {
        if (list !== "ul") (close(), out.push("<ul>"), (list = "ul"));
        out.push(`<li>${inline(m[1])}</li>`);
      } else if ((m = line.match(/^\s*\d+[.)]\s+(.*)$/))) {
        if (list !== "ol") (close(), out.push("<ol>"), (list = "ol"));
        out.push(`<li>${inline(m[1])}</li>`);
      } else if ((m = line.match(/^#{1,4}\s+(.*)$/))) {
        close();
        out.push(`<h4>${inline(m[1])}</h4>`);
      } else if (line.trim()) {
        close();
        out.push(`<p>${inline(line)}</p>`);
      } else close();
    }
    close();
  });
  let html = out.join("");
  if (streaming) {
    const caret = '<span class="caret"></span>';
    html = /<\/(p|li|h4)>$/.test(html) ? html.replace(/<\/(p|li|h4)>$/, caret + "</$1>") : html + caret;
  }
  return html;
}

// ───────── rendering ─────────
function whoHtml(a) {
  const e = ENGINES[a.engine] || ENGINES.groq;
  const meta = a.meta ? `<span class="meta">· ${esc(a.meta)}</span>` : "";
  const label = a.model ? `${e.name} · ${a.model}` : e.name;
  return `<div class="who"><i style="background:${e.color}"></i>${esc(label)}${meta}</div>`;
}
function stepsHtml(a) {
  if (!a.steps?.length) return "";
  const collapsed = a.status !== "running" && !a.showSteps && a.steps.length > 1;
  const more = a.steps.length > 1 && a.status !== "running" ? `<button class="more" data-act="steps">${a.showSteps ? T.steps_less : `${T.steps_more} (${a.steps.length})`}</button>` : "";
  const list = a.status === "running" ? a.steps.slice(-6) : a.steps;
  return `<div class="steps${collapsed ? " collapsed" : ""}">${list.map((s, i) => `<div${i === list.length - 1 ? ' class="last"' : ""}>${esc(s)}</div>`).join("")}${more}</div>`;
}
function answerHtml(a, opts = {}) {
  a._opts = opts;
  let body = "";
  if (a.status === "running" && !a.text) body = `<div class="wait"><span class="spin"></span>${esc(a.mode === "delegate" ? T.working : T.thinking)}</div>`;
  const shown = typed(a);
  const live = a.status === "running" || shown.length < a.text.length;
  if (shown) body += `<div class="body">${md(shown, live)}</div>`;
  if (a.images?.length) {
    const src = (p) => (TAURI?.core?.convertFileSrc ? TAURI.core.convertFileSrc(p) : p);
    body += `<div class="imgs">${a.images.map((p) => `<a class="img" data-act="img" data-path="${esc(p)}" title="${esc(p)}"><img src="${esc(src(p))}" alt="" loading="lazy" /></a>`).join("")}</div>`;
  }
  if (a.status === "error")
    body += a.auth
      ? `<div class="err">${esc(T.err_auth)}<div class="acts"><button class="primary" data-act="relogin">${esc(T.relogin)}</button></div></div>`
      : `<div class="err">${esc(a.error)}</div>`;
  let acts = "";
  if (!live && a.text && !opts.noActs) {
    const open = openMenuFor === a.key;
    const extra = opts.compactActs || a.mode === "delegate" ? "" : `<button data-act="open-chatgpt">${T.open_chatgpt}</button><button data-act="open-claude">${T.open_claude}</button>`;
    acts = `<div class="mact${open ? " open" : ""}">
      <button class="ib" data-act="copy" title="${esc(T.copy)}"><svg viewBox="0 0 24 24"><rect x="8.5" y="8.5" width="11" height="11" rx="2.5"/><path d="M15.5 8.5V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v7.5a2 2 0 0 0 2 2h2.5"/></svg></button>
      <button class="ib" data-act="more" title="${esc(T.more)}"><svg viewBox="0 0 24 24"><circle cx="5.5" cy="12" r="1.3"/><circle cx="12" cy="12" r="1.3"/><circle cx="18.5" cy="12" r="1.3"/></svg></button>
      ${open ? `<div class="mmenu"><button data-act="insert">${esc(T.insert)}<span class="k">Ctrl+↵</span></button><button data-act="copy">${esc(T.copy)}</button>${extra}</div>` : ""}
    </div>`;
  }
  return `<div class="a" data-ans="${a.key}">${whoHtml(a)}${stepsHtml(a)}${body}${acts}</div>`;
}

// Smooth reveal ("typewriter"): show what arrived a few characters at a time.
let openMenuFor = null;
function typed(a) {
  if (!S.ask_typewriter) return a.text;
  if (a.typed == null) a.typed = a.status === "running" ? 0 : a.text.length;
  return a.text.slice(0, a.typed);
}
let twRaf = 0;
// Background of the chat (Settings → Obecné → Pozadí)
function hexA(hex, a) {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex || "");
  const n = m ? parseInt(m[1], 16) : 0x0e0e0f;
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${a})`;
}
function applyLook() {
  const L = S.look || {};
  const op = Math.min(100, Math.max(30, L.opacity ?? 100)) / 100;
  const bg = $("#look");
  bg.style.background = hexA(L.color || "#0E0E0F", op);
  const img = $("#look-img");
  const src = L.image && TAURI?.core?.convertFileSrc ? TAURI.core.convertFileSrc(L.image) : "";
  img.style.backgroundImage = src ? `url("${src}")` : "none";
  img.style.filter = L.blur ? `blur(${L.blur}px)` : "none";
  img.style.transform = L.blur ? "scale(1.08)" : "none";
  img.style.opacity = src ? op : 0;
  $("#look-dim").style.background = src ? hexA(L.color || "#0E0E0F", ((L.dim ?? 35) / 100) * op) : "transparent";
  // plain colour + blur → frosted glass (Windows blurs the desktop behind)
  const glass = !src && (L.blur || 0) > 0 && op < 1;
  $("#shell").classList.toggle("glass", glass);
  if (glass !== glassOn) {
    glassOn = glass;
    invoke("set_glass", { window: "ask", on: glass }).catch(() => {});
  }
}
let glassOn = null;

// user-tunable speeds (Settings → Ask)
const animMs = () => Math.min(2000, Math.max(150, Number(S.ask_anim_ms) || 620));
const typeCps = () => Math.min(1000, Math.max(10, Number(S.ask_type_cps) || 90));
function applyAnim() {
  document.documentElement.style.setProperty("--expand-ms", animMs() + "ms");
}
let twLast = 0;
// While the popup is closing or hidden, don't paint at all: answers keep
// arriving (a.text grows), they're drawn when the chat shows again. Painting
// a streaming answer during the close animation is what made closing stutter.
const paintPaused = () => !EMBED && (closing || !shown);
function typewriterTick(now) {
  twRaf = 0;
  if (paintPaused()) return;
  const dt = twLast ? Math.min(100, now - twLast) : 16;
  twLast = now;
  let more = false;
  const all = [];
  for (const it of conv.items) {
    if (it.type === "answer") all.push(it);
    else if (it.type === "pair") all.push(it.a, it.b);
    else if (it.type === "synth") all.push(it.a);
  }
  const changed = [];
  for (const a of all) {
    if (!a || a.typed == null || a.typed >= a.text.length) continue;
    const backlog = a.text.length - a.typed;
    // chosen speed; speeds up only if it falls more than ~4 s behind
    let step = (typeCps() * dt) / 1000;
    if (backlog > typeCps() * 4) step *= backlog / (typeCps() * 4);
    a.typedF = (a.typedF ?? a.typed) + step;
    a.typed = Math.min(a.text.length, Math.floor(a.typedF));
    more = true;
    changed.push(a);
  }
  if (changed.length && !changed.every(patchAnswer)) render();
  else if (!more) render();
  if (more) twRaf = requestAnimationFrame(typewriterTick);
  else twLast = 0;
}
function kickTypewriter() {
  if (paintPaused()) return;
  if (S.ask_typewriter && !twRaf) twRaf = requestAnimationFrame(typewriterTick);
}

function render() {
  const thread = $("#thread");
  const atBottom = thread.scrollHeight - thread.scrollTop - thread.clientHeight < 60;
  let html = "";
  if (!conv.items.length) {
    html = `<div class="empty-state">${conv.mode === "debate" ? T.empty_debate : conv.mode === "delegate" ? T.empty_delegate : T.empty_ask}</div>`;
  }
  for (const it of conv.items) {
    if (it.type === "user") {
      const note = it.ctx ? `<span class="ctxnote">📎 ${esc(T.sel)}${it.ctxApp ? ` ${T.from} ${esc(it.ctxApp)}` : ""}</span>` : "";
      html += `<div class="u">${note}${esc(it.text)}</div>`;
    } else if (it.type === "answer") {
      html += answerHtml(it);
    } else if (it.type === "pair") {
      if (it.round > 1) html += `<div class="round">${T.round} ${it.round}</div>`;
      html += `<div class="pair">${answerHtml(it.a, { compactActs: true })}${answerHtml(it.b, { compactActs: true })}</div>`;
      const last = conv.items[conv.items.length - 1] === it;
      if (last && it.a.status !== "running" && it.b.status !== "running" && (it.a.text || it.b.text)) {
        html += `<div class="acts" style="margin:-2px 0 14px"><button class="primary" data-act="debate-more">${T.debate_more}</button><button data-act="debate-sum">${T.debate_sum}</button></div>`;
      }
    } else if (it.type === "synth") {
      html += `<div class="round">${T.synth}</div><div class="synth">${answerHtml(it.a)}</div>`;
    }
  }
  thread.innerHTML = html;
  if (atBottom) thread.scrollTop = thread.scrollHeight;
  renderChrome();
}
let raf = 0;
const dirty = new Set();
function renderSoon(a) {
  if (a) dirty.add(a);
  else dirty.add("*");
  if (raf || paintPaused()) return;
  raf = requestAnimationFrame(() => {
    raf = 0;
    const all = dirty.has("*");
    const list = [...dirty];
    dirty.clear();
    if (all || !list.every(patchAnswer)) render();
  });
}
// Re-draw one answer in place (cheap) — false if it isn't on screen yet.
function patchAnswer(a) {
  if (!a || typeof a !== "object") return false;
  const el = $(`#thread [data-ans="${a.key}"]`);
  if (!el) return false;
  const thread = $("#thread");
  const atBottom = thread.scrollHeight - thread.scrollTop - thread.clientHeight < 60;
  el.outerHTML = answerHtml(a, a._opts || {});
  if (atBottom) thread.scrollTop = thread.scrollHeight;
  return true;
}

// Hold Shift+Backspace: talk straight into the chat
let voiceState = "";
function setVoice(st) {
  voiceState = st;
  $("#composer").classList.toggle("listening", st === "listening");
  $("#composer").classList.toggle("transcribing", st === "thinking");
  renderChrome();
}
listen("whiskr:ask-voice", (ev) => {
  const p = ev.payload || {};
  dbg(`ask-voice ${p.state}`);
  if (p.state === "listening" || p.state === "thinking") return setVoice(p.state);
  setVoice("");
  if (p.state === "text" && p.text) {
    const cur = input.value.trim();
    input.value = cur ? `${cur} ${p.text}` : p.text;
    autosize();
    renderChrome();
    if (S.ask_voice_send !== false) send();
    else input.focus();
  } else if (p.state === "empty") {
    const ph = input.placeholder;
    input.placeholder = T.v_empty;
    setTimeout(() => renderChrome(), 1800);
    void ph;
  } else if (p.state === "error" && p.text) {
    input.placeholder = String(p.text).slice(0, 80);
    setTimeout(() => renderChrome(), 3000);
  }
});

function renderChrome() {
  for (const b of document.querySelectorAll("#modes button")) b.classList.toggle("on", b.dataset.mode === conv.mode);
  const e = ENGINES[conv.engine] || ENGINES.claude;
  $("#engine i").style.background = e.color;
  $("#engine span").textContent = conv.mode === "debate" ? debatePair().map((x) => ENGINES[x]?.name || "?").join(" × ") : modelName(conv.engine);
  $("#input").placeholder = voiceState === "listening" ? T.v_listen : voiceState === "thinking" ? T.v_think : conv.mode === "debate" ? T.ph_debate : conv.mode === "delegate" ? T.ph_delegate : T.ph_ask;
  const busy = [...runs.values()].some((a) => a.conv === conv.id);
  void busy;
  $("#send").classList.toggle("ready", !!$("#input").value.trim() || !!selection);
  const bg = [...runs.values()].filter((a) => a.mode === "delegate").length;
  $("#runs").hidden = !bg;
  $("#runs").innerHTML = bg ? `<span class="spin"></span>${bg} ${T.runs}` : "";
  const prof = effectiveProfile();
  $("#profile").textContent = "👤 " + prof.name;
  $("#profile").classList.toggle("work", !!prof.key);
  $("#profile").hidden = profiles().length < 2;
  $("#pin-btn").classList.toggle("on", pinned);
  renderCtx();
  renderHint();
  requestAnimationFrame(() => fitCompact($("#engine-menu").hidden ? 0 : $("#engine-menu").offsetHeight + 10));
}

function renderCtx() {
  let html = "";
  if (selection) {
    const n = selection.text.length;
    html += `<span class="chip">📎 <b>${esc(T.sel)}</b> ${n} zn.${selection.app ? ` · ${esc(selection.app.slice(0, 30))}` : ""}<button class="x" data-act="drop-sel">✕</button></span>`;
  }
  if (conv.mode === "delegate") {
    const opts = (S.projects || []).map((p) => `<option value="${esc(p.path)}"${p.path === conv.cwd ? " selected" : ""}>${esc(p.name)}</option>`).join("");
    html += `<select id="proj"><option value="">${esc(T.no_project)}</option>${opts}<option value="__add__">${esc(T.add_project)}</option></select>`;
    const full = S.ask_access === "full";
    html += `<button class="chip${full ? " warn" : ""}" data-act="access" title="${esc(full ? T.full_t : T.safe_t)}">${full ? T.full : T.safe}</button>`;
  }
  $("#ctx").innerHTML = html;
}

function renderHint() {
  return; // minimal window: no shortcut row
}

// ───────── layout ─────────
const nextFrame = () => new Promise((r) => requestAnimationFrame(() => r()));
let layoutSeq = 0;
let compactH = 0;

// Collapsed: the window is exactly the bar. Grows upward (bottom edge stays).
// Windows: the window keeps its full size; we clip it to what's visible.
const IS_WIN = !EMBED && /Windows/i.test(navigator.userAgent);
let lastRegion = "";
function setRegion(x, y, w, h) {
  // +2: Windows cuts the region with hard (pixelated) edges, so it stays a
  // little outside the panel and the page's own smooth corners show
  const r = ($("#shell").classList.contains("glass") ? 8 : 22) + 2;
  const key = [x, y, w, h, r].map(Math.round).join(",");
  if (key === lastRegion) return;
  lastRegion = key;
  invoke("ask_region", { x, y, w, h, r }).catch(() => {});
}
// region = the panel (+ the model menu when it's open above it)
function updateRegion() {
  if (!IS_WIN) return;
  // layout box (ignores the appear animation's scale)
  const el = $("#panel");
  if (!el.offsetHeight) return;
  let x1 = el.offsetLeft, y1 = el.offsetTop, x2 = x1 + el.offsetWidth, y2 = y1 + el.offsetHeight;
  const m = $("#engine-menu");
  if (!m.hidden) {
    const r = m.getBoundingClientRect();
    x1 = Math.min(x1, r.left); y1 = Math.min(y1, r.top); x2 = Math.max(x2, r.right); y2 = Math.max(y2, r.bottom);
  }
  setRegion(x1 - 2, y1 - 2, x2 - x1 + 4, y2 - y1 + 4);
}
window.addEventListener("resize", () => requestAnimationFrame(updateRegion));

function fitCompact(extra = 0) {
  if ($("#shell").dataset.state !== "compact") return;
  if (IS_WIN) return requestAnimationFrame(updateRegion);
  // composer = the whole collapsed panel (+2 border, +6 window padding)
  const inner = $("#composer").scrollHeight;
  if (!inner) return; // hidden window: no layout yet
  const h = Math.max(60, Math.ceil(inner + 2 + 6 + extra));
  if (h === compactH) return;
  compactH = h;
  invoke("ask_layout", { expanded: false, height: h }).catch(() => {});
}

async function setExpanded(on, animate = true) {
  if (EMBED) return;
  const shell = $("#shell");
  const panel = $("#panel");
  const st = on ? "expanded" : "compact";
  if (shell.dataset.state === st) return;
  const seq = ++layoutSeq;
  if (!on) {
    shell.dataset.state = "compact";
    panel.classList.remove("anim");
    panel.style.height = "";
    renderChrome();
    await nextFrame();
    compactH = 0;
    fitCompact();
    return;
  }
  if (IS_WIN) {
    // No resize at all: open the clip to the full window, then grow the panel.
    const from = panel.getBoundingClientRect().height;
    const fromW = panel.getBoundingClientRect().width;
    panel.style.height = from + "px";
    panel.style.maxWidth = fromW + "px";
    shell.dataset.state = "expanded";
    renderChrome();
    setRegion(1, 1, window.innerWidth - 2, window.innerHeight - 2);
    if (!animate) {
      panel.style.height = panel.style.maxWidth = "";
      return;
    }
    panel.offsetHeight;
    panel.classList.add("anim");
    panel.style.height = shell.clientHeight - 6 + "px";
    panel.style.maxWidth = shell.clientWidth - 6 + "px";
    const doneW = () => {
      if (seq !== layoutSeq) return;
      panel.classList.remove("anim");
      panel.style.height = panel.style.maxWidth = "";
      const th = $("#thread");
      th.scrollTop = th.scrollHeight;
      updateRegion();
    };
    panel.addEventListener("transitionend", doneW, { once: true });
    setTimeout(doneW, animMs() + 150);
    return;
  }
  // Grow the window first (instantly, upward), then let the panel slide up.
  const from = panel.getBoundingClientRect().height;
  const before = window.innerHeight;
  panel.style.height = from + "px";
  shell.dataset.state = "expanded";
  renderChrome();
  await invoke("ask_layout", { expanded: true, height: null }).catch(() => {});
  if (before < 300) for (let i = 0; i < 30 && window.innerHeight === before; i++) await nextFrame();
  else await nextFrame();
  if (seq !== layoutSeq) return;
  const to = shell.clientHeight - 6;
  if (!animate || !TAURI_ANIM) {
    panel.style.height = "";
    return;
  }
  panel.offsetHeight;
  panel.classList.add("anim");
  panel.style.height = to + "px";
  const done = () => {
    if (seq !== layoutSeq) return;
    panel.classList.remove("anim");
    panel.style.height = "";
    const th = $("#thread");
    th.scrollTop = th.scrollHeight;
  };
  panel.addEventListener("transitionend", done, { once: true });
  setTimeout(done, animMs() + 150);
}
const TAURI_ANIM = true;

function autosize() {
  const ta = $("#input");
  if (!ta.offsetParent) return renderChrome(); // not laid out (window hidden)
  ta.style.height = "auto";
  ta.style.height = Math.min(160, ta.scrollHeight) + "px";
  renderChrome();
}

// ───────── running ─────────
function sessionKey(engine) {
  return `${engine}:${effectiveProfile().id}:${conv.mode}:${conv.cwd || ""}`;
}

let keySeq = 0;
async function startRun(engine, prompt, { resume = true, history = [], mode } = {}) {
  const a = { key: ++keySeq, conv: conv.id, engine, mode: mode || conv.mode, text: "", steps: [], status: "running", meta: "", model: modelFor(engine) ? modelName(engine) : "" };
  const prof = effectiveProfile();
  if (prof.key && engine !== "groq") a.meta = prof.name;
  const sk = sessionKey(engine);
  a.sessionKey = sk;
  const target = conv;
  try {
    const run = await invoke("ask_run", {
      req: {
        engine,
        mode: a.mode === "delegate" ? "delegate" : "ask",
        prompt,
        cwd: a.mode === "delegate" ? conv.cwd : null,
        resume: resume ? conv.sessions[sk] || null : null,
        profile: prof.id,
        history,
        model: modelFor(engine) || null,
      },
    });
    a.run = run;
    a.target = target;
    runs.set(run, a);
  } catch (e) {
    a.status = "error";
    a.error = String(e);
  }
  return a;
}

listen("whiskr:agent", (ev) => {
  const p = ev.payload;
  const a = runs.get(p.run);
  if (!a) return;
  switch (p.kind) {
    case "delta":
      a.text += p.text;
      break;
    case "text":
      a.text += (a.text && !a.text.endsWith("\n") ? "\n\n" : "") + p.text;
      break;
    case "step":
      a.steps.push(p.label);
      break;
    case "image":
      (a.images ||= []).push(p.path);
      break;
    case "session":
      if (a.target) a.target.sessions[a.sessionKey] = p.id;
      break;
    case "done":
      if (!a.text.trim() && p.text) a.text = p.text;
      a.status = "done";
      runs.delete(p.run);
      finished(a);
      break;
    case "error":
      a.status = "error";
      a.error = p.text;
      if (a.text.trim() === String(p.text || "").trim()) a.text = "";
      if (/oauth|authenticat|log ?in|401|unauthori/i.test(p.text || "")) a.auth = true;
      runs.delete(p.run);
      finished(a);
      break;
  }
  if (a.conv === conv.id) {
    const structural = p.kind === "done" || p.kind === "error";
    if (S.ask_typewriter && !structural) kickTypewriter();
    else if (structural) {
      renderSoon();
      kickTypewriter();
    }
    else renderSoon(a);
  } else renderChrome();
});

function finished(a) {
  if (a.target) {
    a.target.updated = Date.now();
    persist(a.target);
  }
}

async function runSlash(cmd, arg) {
  $("#input").value = "";
  autosize();
  if (cmd === "brief") {
    if (!arg) return;
    if (conv.mode !== "ask") setMode("ask");
    if (conv.items.length) fresh(false);
    setExpanded(true);
    conv.title = `📋 ${arg}`;
    conv.items.push({ type: "user", text: `📋 Brief: ${arg}` });
    const slot = { type: "answer", key: ++keySeq, engine: conv.engine, text: "", steps: [], status: "running" };
    conv.items.push(slot);
    render();
    let mat;
    try {
      mat = await invoke("brief_material", { query: arg });
    } catch (e) {
      Object.assign(slot, { status: "error", error: String(e) });
      return render();
    }
    if (!available(conv.engine) || conv.engine === "cursor") conv.engine = firstAvailable(["cursor"]) || conv.engine;
    slot.steps = mat.titles.map((t) => "📼 " + t);
    const a = await startRun(conv.engine, mat.prompt, { resume: false });
    a.steps = slot.steps;
    replaceAnswer(slot, a);
    return render();
  }
  // tasks
  if (!arg) return;
  try {
    await invoke("add_task", { text: arg, smart: true, source: "ask", sourceId: null });
    setExpanded(true);
    conv.items.push({ type: "user", text: `/úkol ${arg}` });
    conv.items.push({ type: "answer", key: ++keySeq, engine: "whiskr", text: T.task_added, steps: [], status: "done" });
    render();
  } catch (e) {
    conv.items.push({ type: "answer", key: ++keySeq, engine: "groq", text: "", steps: [], status: "error", error: String(e) });
    render();
  }
}

function buildPrompt(text) {
  let q = text.trim();
  if (selection) {
    q = `${q || T.sel_only}\n\n<oznaceny_text${selection.app ? ` zdroj="${selection.app.replace(/"/g, "'")}"` : ""}>\n${selection.text}\n</oznaceny_text>`;
  }
  return q;
}

async function send() {
  let text = $("#input").value;
  if (!text.trim() && !selection) return;
  // @model prefix
  const m = text.match(/^\s*@(gpt|codex|claude|cursor|groq)\b\s*/i);
  if (m) {
    const k = m[1].toLowerCase();
    conv.engine = k === "gpt" ? "codex" : k;
    text = text.slice(m[0].length);
  }
  // Slash commands: /brief <who>, /úkol <what>
  const slash = text.match(/^\s*\/(brief|úkol|ukol|task)\b\s*([\s\S]*)$/i);
  if (slash) return runSlash(slash[1].toLowerCase(), slash[2].trim());
  const prompt = buildPrompt(text);
  const user = { type: "user", text: text.trim() || "📎", ctx: !!selection, ctxApp: selection?.app };
  if (!conv.title) conv.title = (text.trim() || selection?.text || "").slice(0, 70);
  selection = null;
  $("#input").value = "";
  autosize();
  setExpanded(true);
  conv.items.push(user);

  if (conv.mode === "debate") {
    const [ea, eb] = debatePair();
    if (!eb) {
      conv.items.push({ type: "answer", key: ++keySeq, engine: "groq", text: "", steps: [], status: "error", error: T.need_two });
      return render();
    }
    const pair = { type: "pair", round: 1, question: prompt, a: placeholder(ea), b: placeholder(eb) };
    conv.items.push(pair);
    render();
    [pair.a, pair.b] = await Promise.all([startRun(ea, prompt, { resume: false }), startRun(eb, prompt, { resume: false })]);
  } else if (conv.mode === "delegate") {
    if (!conv.cwd) {
      conv.items.push({ type: "answer", key: ++keySeq, engine: conv.engine, text: "", steps: [], status: "error", error: T.no_project });
      return render();
    }
    if (conv.engine === "groq" || !available(conv.engine)) conv.engine = firstAvailable(["groq"]) || conv.engine;
    const slot = { type: "answer", key: ++keySeq, engine: conv.engine, text: "", steps: [], status: "running", mode: "delegate" };
    conv.items.push(slot);
    render();
    replaceAnswer(slot, await startRun(conv.engine, prompt));
  } else {
    if (!available(conv.engine)) conv.engine = firstAvailable() || conv.engine;
    const history = conv.engine === "groq" ? historyPairs() : [];
    const slot = { type: "answer", key: ++keySeq, engine: conv.engine, text: "", steps: [], status: "running" };
    conv.items.push(slot);
    render();
    const a = await startRun(conv.engine, prompt, { history });
    replaceAnswer(slot, a);
  }
  render();
}

// keep the same object in `items` that the run map updates
function replaceAnswer(slot, a) {
  const i = conv.items.indexOf(slot);
  if (i >= 0) conv.items[i] = Object.assign(a, { type: "answer" });
}

function historyPairs() {
  const out = [];
  let q = null;
  for (const it of conv.items) {
    if (it.type === "user") q = it.text;
    else if (it.type === "answer" && q && it.text) (out.push([q, it.text]), (q = null));
  }
  return out;
}

const placeholder = (engine) => ({ key: ++keySeq, engine, text: "", steps: [], status: "running" });
function debatePair() {
  const a = available("claude") ? "claude" : firstAvailable();
  const b = available("codex") && a !== "codex" ? "codex" : firstAvailable([a]);
  return [a, b];
}

async function debateMore() {
  const last = conv.items[conv.items.length - 1];
  if (last?.type !== "pair") return;
  const [ea, eb] = [last.a.engine, last.b.engine];
  const pair = { type: "pair", round: last.round + 1, question: last.question, a: placeholder(ea), b: placeholder(eb) };
  conv.items.push(pair);
  render();
  [pair.a, pair.b] = await Promise.all([
    startRun(ea, T.debate_prompt(ENGINES[eb].name, last.b.text || "(bez odpovědi)")),
    startRun(eb, T.debate_prompt(ENGINES[ea].name, last.a.text || "(bez odpovědi)")),
  ]);
  render();
}

async function debateSum() {
  const last = conv.items[conv.items.length - 1];
  if (last?.type !== "pair") return;
  const q = conv.items.find((x) => x.type === "pair")?.question || "";
  const synth = { type: "synth" };
  conv.items.push(synth);
  synth.a = { key: ++keySeq, engine: last.a.engine, text: "", steps: [], status: "running" };
  render();
  synth.a = await startRun(last.a.engine, T.sum_prompt(q, last.a.text, ENGINES[last.a.engine].name, last.b.text, ENGINES[last.b.engine].name), { resume: false });
  render();
}

function findAnswer(key) {
  for (const it of conv.items) {
    if (it.type === "answer" && it.key == key) return it;
    if (it.type === "pair") {
      if (it.a?.key == key) return it.a;
      if (it.b?.key == key) return it.b;
    }
    if (it.type === "synth" && it.a?.key == key) return it.a;
  }
}
function lastAnswer() {
  for (let i = conv.items.length - 1; i >= 0; i--) {
    const it = conv.items[i];
    if (it.type === "answer" && it.text) return it;
    if (it.type === "synth" && it.a?.text) return it.a;
    if (it.type === "pair" && (it.a?.text || it.b?.text)) return it.a?.text ? it.a : it.b;
  }
}
function questionFor(a) {
  let q = "";
  for (const it of conv.items) {
    if (it.type === "user") q = it.text;
    if (it === a || it.a === a || it.b === a) break;
  }
  return q;
}

function stopAll() {
  for (const [run, a] of runs) if (a.conv === conv.id) invoke("ask_cancel", { run });
}

// ───────── persistence ─────────
function slim(c) {
  const strip = (a) => a && { key: a.key, engine: a.engine, mode: a.mode, text: a.text, images: a.images, steps: (a.steps || []).slice(-30), status: a.status === "running" ? "error" : a.status, error: a.status === "running" ? "Přerušeno" : a.error, meta: a.meta };
  return {
    ...c,
    items: c.items.map((it) => (it.type === "answer" ? { ...strip(it), type: "answer" } : it.type === "pair" ? { ...it, a: strip(it.a), b: strip(it.b) } : it.type === "synth" ? { type: "synth", a: strip(it.a) } : it)),
  };
}
let saveTimer = 0;
function persist(c) {
  if (!c.items.length) return;
  const i = convs.findIndex((x) => x.id === c.id);
  if (i >= 0) convs.splice(i, 1);
  convs.unshift(c);
  convs = convs.slice(0, 80);
  renderSide();
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => invoke("ask_store_set", { data: JSON.stringify({ convs: convs.map(slim) }) }).catch(() => {}), 300);
}
async function loadStore() {
  try {
    const d = JSON.parse(await invoke("ask_store_get"));
    convs = d.convs || [];
  } catch {
    convs = [];
  }
}

let sideOpen = false; // the side panel starts folded every time
function toggleSide() {
  sideOpen = !sideOpen;
  try {
    localStorage.setItem("whiskr-side", sideOpen ? "1" : "0");
  } catch {}
  if (sideOpen) setExpanded(true);
  $("#shell").classList.toggle("side-open", sideOpen);
  $("#side-btn").classList.toggle("on-side", sideOpen);
  renderSide();
}
const MODE_ICON = {
  ask: '<svg viewBox="0 0 24 24"><path d="M5 5.5h14v10H10l-5 4z"/></svg>',
  debate: '<svg viewBox="0 0 24 24"><path d="M4 5h10v7H8l-4 3zM20 10v8l-3-2h-6v-2"/></svg>',
  delegate: '<svg viewBox="0 0 24 24"><path d="M4 18l6-6-6-6M12 18h8"/></svg>',
};
function renderSide() {
  renderSideInner();
  // embedded: the main window's sidebar shows the chat list
  if (EMBED) toParent({ chats: $("#side-list")?.innerHTML || "", ph: T.search, nl: T.new_chat });
}
function renderSideInner() {
  const box = $("#side-list");
  if (!box || !sideOpen) return;
  const q = $("#side-search").value.trim().toLowerCase();
  let list = convs.slice();
  if (conv.items.length && !list.includes(conv)) list.unshift(conv);
  if (q) list = list.filter((c) => (c.title || "").toLowerCase().includes(q) || c.items.some((it) => it.type === "user" && (it.text || "").toLowerCase().includes(q)));
  if (!list.length) return (box.innerHTML = `<div class="side-empty">${esc(q ? "—" : T.hist_empty)}</div>`);
  const now = new Date();
  const day0 = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const group = (t) => (t >= day0 ? "g_today" : t >= day0 - 864e5 ? "g_yesterday" : t >= day0 - 6 * 864e5 ? "g_week" : "g_older");
  let html = "";
  let last = "";
  for (const c of list) {
    const g = group(c.updated || 0);
    if (g !== last) (html += `<div class="side-g">${esc(T[g])}</div>`), (last = g);
    const busy = [...runs.values()].some((a) => a.conv === c.id);
    html += `<button class="side-item${c === conv ? " on" : ""}" data-conv="${esc(c.id)}" title="${esc(c.title || "")}">
      <span class="ic">${MODE_ICON[c.mode] || MODE_ICON.ask}</span><span class="t">${esc(c.title || "…")}</span>${busy ? '<span class="busy"></span>' : ""}
      <span class="del" data-del="${esc(c.id)}" title="${esc(T.del)}"><svg viewBox="0 0 24 24"><path d="M6.5 6.5l11 11M17.5 6.5l-11 11"/></svg></span></button>`;
  }
  box.innerHTML = html;
}
function saveStore() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => invoke("ask_store_set", { data: JSON.stringify({ convs: convs.map(slim) }) }).catch(() => {}), 300);
}
function renderHist() {
  return renderSide();
}

// ───────── settings ─────────
let sLoaded = false;
async function loadSettings() {
  for (let i = 0; i < 40; i++) {
    try {
      S = await invoke("get_settings");
      sLoaded = true;
      break;
    } catch {
      if (sLoaded) break; // keep what we have
      await new Promise((r) => setTimeout(r, 150));
    }
  }
  T = L[S.ui_lang] || L.cs;
  applyAnim();
  applyLook();
  $("#m-ask").textContent = T.m_ask;
  $("#m-debate").textContent = T.m_debate;
  $("#m-delegate").textContent = T.m_delegate;
  $("#new-label").textContent = T.new_chat;
  $("#side-app-label").textContent = T.dashboard;
  $("#side-set-label").textContent = T.settings;
  $("#app-btn").title = T.open_app + " (Ctrl+,)";
  $("#side-search").placeholder = T.search;
  $("#pin-btn").title = T.pin;
  $("#profile").title = T.profile_t;
}
async function saveSettings(patch) {
  S = { ...S, ...patch };
  if (!sLoaded) return; // never overwrite settings we haven't read
  try {
    S = await invoke("save_settings", { settings: S });
  } catch (e) {
    console.warn(e);
  }
}
async function loadTools(refresh = false) {
  try {
    tools = await invoke("ask_tools", { refresh, checkLogin: false });
  } catch {}
}

// ───────── events ─────────
const input = $("#input");
input.addEventListener("input", autosize);
input.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey && !e.ctrlKey) {
    e.preventDefault();
    send();
  }
});
document.addEventListener("click", (e) => {
  if (openMenuFor != null && !e.target.closest(".mact")) {
    openMenuFor = null;
    render();
  }
});
// Shift+Space inside the chat closes it too (the system hook may not see it
// while our window has focus). Same guard as the hook: only when Shift went
// down on its own — typing "A " fast never closes anything.
let shiftAt = 0;
let focusAt = 0;
window.addEventListener("focus", () => (focusAt = performance.now()));
let typedSinceShift = false;
window.addEventListener(
  "keydown",
  (e) => {
    if (e.key === "Shift") {
      if (!e.repeat) {
        shiftAt = performance.now();
        typedSinceShift = false;
      }
      return;
    }
    if (e.code === "Space" && e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey) {
      // Shift went down before we got the keyboard = the chord that OPENED
      // the chat is still held (its key repeat lands here) — never close on it.
      if (shiftAt < focusAt) {
        e.preventDefault();
        return;
      }
      if (!typedSinceShift && !e.repeat) {
        e.preventDefault();
        e.stopPropagation();
        hide("page Shift+Space");
      }
      return;
    }
    typedSinceShift = true;
  },
  true
);

// Shift+Backspace: drop the window right away (the draft stays) — caught
// before the text box sees it, so it works while typing too.
window.addEventListener(
  "keydown",
  (e) => {
    // On Windows the system hook handles Shift+Backspace (tap = close,
    // hold = talk). This is only the fallback elsewhere — and never when
    // you typed with Shift held (then it's just fixing a typo).
    if (IS_WIN || EMBED) return;
    if ((e.code === "Backspace" || e.key === "Backspace") && e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey && !typedSinceShift) {
      e.preventDefault();
      e.stopPropagation();
      hide("Shift+Backspace");
    }
  },
  true
);
const openApp = () => invoke("open_main", { page: null }).catch(() => {});
$("#app-btn").addEventListener("click", openApp);
$("#side-app").addEventListener("click", () => invoke("open_main", { page: "home" }).catch(() => {}));
$("#side-set").addEventListener("click", () => invoke("open_main", { page: "settings" }).catch(() => {}));
document.addEventListener("keydown", (e) => {
  // Shift+Backspace is handled in the capture listener below
  if (false) {
    e.preventDefault();
    return hide();
  }
  if (e.ctrlKey && e.key === ",") {
    e.preventDefault();
    return openApp();
  }
  if (e.key === "Escape") {
    e.preventDefault();
    if (!$("#engine-menu").hidden) return closeMenu();
    hide("Esc");
  } else if (e.ctrlKey && e.key === "Enter") {
    e.preventDefault();
    const a = lastAnswer();
    if (a) invoke("ask_insert", { text: a.text });
  } else if (e.ctrlKey && ["1", "2", "3"].includes(e.key)) {
    e.preventDefault();
    setMode(["ask", "debate", "delegate"][Number(e.key) - 1]);
  } else if (e.ctrlKey && (e.key === "n" || e.key === "N")) {
    e.preventDefault();
    fresh(true);
  } else if (e.ctrlKey && (e.key === "h" || e.key === "H" || e.key === "b" || e.key === "B")) {
    e.preventDefault();
    toggleSide();
  }
});

function setMode(mode) {
  if (conv.mode === mode) return;
  if (conv.items.length) {
    persist(conv);
    const keep = { engine: conv.engine };
    conv = newConv();
    conv.engine = keep.engine;
  }
  conv.mode = mode;
  if (mode === "delegate") {
    if (conv.engine === "groq") conv.engine = firstAvailable(["groq"]) || "claude";
    conv.cwd = S.projects?.[0]?.path || "";
  }
  render();
  input.focus();
}

$("#modes").addEventListener("click", (e) => {
  const b = e.target.closest("[data-mode]");
  if (b) setMode(b.dataset.mode);
});

$("#send").addEventListener("click", () => send());

// Closing = the opening played backwards: the chat sinks back towards the bar
// and fades, then the window really hides.
let closing = false;
let closeTimer = 0;
let closeAt = 0;
function cancelClose() {
  dbg("cancelClose");
  invoke("ask_close_cancel").catch(() => {});
  clearTimeout(closeTimer);
  closing = false;
  const panel = $("#panel");
  panel.classList.remove("leave");
  panel.style.opacity = "1";
  void panel.offsetWidth;
  panel.style.opacity = "";
  panel.classList.remove("enter");
  void panel.offsetWidth;
  panel.classList.add("enter");
  input.focus();
  render();
  kickTypewriter();
}
let shown = false; // the popup is on screen (between "show" and "hidden")
function hide(why = "") {
  if (EMBED) return toParent({ goto: "home" });
  dbg(`hide(${why}) closing=${closing} shown=${shown}`);
  if (closing || !shown) return;
  closing = true;
  closeAt = performance.now();
  const panel = $("#panel");
  // closing is always quick, whatever the opening speed is set to
  const ms = Math.min(220, Math.max(150, Math.round(animMs() * 0.35)));
  panel.style.setProperty("--leave-ms", ms + "ms");
  panel.classList.remove("enter", "away");
  panel.classList.add("leave");
  if (raf) cancelAnimationFrame(raf), (raf = 0);
  if (twRaf) cancelAnimationFrame(twRaf), (twRaf = 0);
  // the core hides the window after `ms` — on time even if this page is busy
  invoke("ask_hide_after", { ms }).catch(() => invoke("ask_hide").catch(() => {}));
  // never stay "closing" if the window did not really hide
  closeTimer = setTimeout(() => (closing = false), ms + 900);
}
// Shift+Space again while it is closing = changed my mind: stay open.
// (the key can reach us twice — system hook and page — so ignore a repeat within 350 ms)
listen("whiskr:ask-close", () => {
  dbg(`ask-close closing=${closing} shown=${shown}`);
  if (!closing) return hide("hotkey");
  if (performance.now() - closeAt > 120) cancelClose();
});
$("#close-btn").addEventListener("click", () => hide("close button"));
$("#pin-btn").addEventListener("click", () => {
  pinned = !pinned;
  renderChrome();
});
$("#new-btn").addEventListener("click", () => {
  // stay expanded with the side panel: just an empty chat
  if (conv.items.length) persist(conv);
  const engine = conv.engine;
  conv = newConv();
  conv.engine = engine;
  selection = null;
  render();
  renderSide();
  input.focus();
});
$("#side-btn").addEventListener("click", toggleSide);
window.addEventListener("message", (e) => {
  const m = e.data;
  if (!EMBED || !m || !m.whiskr || !m.chatCmd) return;
  const c = m.chatCmd;
  if (c.type === "new") $("#new-btn").click();
  else if (c.type === "open") $(`#side-list [data-conv="${CSS.escape(c.id)}"]`)?.click();
  else if (c.type === "del") $(`#side-list [data-del="${CSS.escape(c.id)}"]`)?.click();
  else if (c.type === "search") { $("#side-search").value = c.q || ""; renderSide(); }
});
$("#side-search").addEventListener("input", renderSide);
$("#side-list").addEventListener("click", (e) => {
  const del = e.target.closest("[data-del]");
  if (del) {
    e.stopPropagation();
    const id = del.dataset.del;
    convs = convs.filter((c) => c.id !== id);
    saveStore();
    if (conv.id === id) {
      conv = newConv();
      render();
    }
    return renderSide();
  }
  const b = e.target.closest("[data-conv]");
  if (!b) return;
  const c = convs.find((x) => x.id === b.dataset.conv);
  if (!c || c === conv) return;
  if (conv.items.length) persist(conv);
  conv = c;
  render();
  renderSide();
  const th = $("#thread");
  th.scrollTop = th.scrollHeight;
});
$("#profile").addEventListener("click", async () => {
  const ps = profiles();
  if (conv.mode === "delegate" && project()?.profile) return; // project decides
  const i = ps.findIndex((p) => p.id === (S.active_profile || "personal"));
  await saveSettings({ active_profile: ps[(i + 1) % ps.length].id });
  renderChrome();
});

// engine menu
function closeMenu() {
  if ($("#engine-menu").hidden) return;
  $("#engine-menu").hidden = true;
  fitCompact();
}
function renderMenu() {
  const menu = $("#engine-menu");
  const list = conv.mode === "delegate" ? ["claude", "codex", "cursor"] : ["claude", "codex", "cursor", "groq"];
  menu.innerHTML = list
    .map((k) => {
      const e = ENGINES[k];
      const ok = available(k);
      const info = k === "groq" ? (ok ? e.sub : T.no_key) : ok ? toolInfo(k) : T.not_installed;
      let models = "";
      if (ok) {
        const ms = modelCache[k];
        const cur = modelFor(k);
        const item = (id, name) =>
          `<button class="mi${k === conv.engine && cur === id ? " on" : ""}" data-engine="${k}" data-model="${esc(id)}">${esc(name)}${k === conv.engine && cur === id ? '<span class="ck">✓</span>' : ""}</button>`;
        models = item("", T.def_model);
        if (ms === "loading") models += `<div class="mloading">${esc(T.loading_models)}</div>`;
        else if (Array.isArray(ms)) models += ms.map((m) => item(m.id, m.name)).join("");
      }
      return `<div class="mg${ok ? "" : " off"}"><div class="mh"><i style="background:${e.color}"></i>${esc(e.name)}<small>${esc(info || "")}</small></div>${models}</div>`;
    })
    .join("");
}
$("#engine").addEventListener("click", async () => {
  const menu = $("#engine-menu");
  if (!menu.hidden) return closeMenu();
  menu.hidden = false;
  renderMenu();
  fitCompact(Math.min(menu.scrollHeight, 380) + 10);
  const list = conv.mode === "delegate" ? ["claude", "codex", "cursor"] : ["claude", "codex", "cursor", "groq"];
  await Promise.all(
    list.filter(available).map(async (k) => {
      await loadModels(k);
      if (!menu.hidden) {
        renderMenu();
        fitCompact(Math.min(menu.scrollHeight, 380) + 10);
      }
    })
  );
});
$("#engine-menu").addEventListener("click", (e) => {
  const b = e.target.closest("[data-engine]");
  if (!b || b.disabled) return;
  conv.engine = b.dataset.engine;
  const models = { ...(S.ask_models || {}), [conv.engine]: b.dataset.model || "" };
  closeMenu();
  saveSettings(conv.mode !== "debate" ? { ask_engine: conv.engine, ask_models: models } : { ask_models: models });
  renderChrome();
  input.focus();
});

$("#ctx").addEventListener("click", async (e) => {
  const b = e.target.closest("[data-act]");
  if (!b) return;
  if (b.dataset.act === "drop-sel") selection = null;
  if (b.dataset.act === "access") await saveSettings({ ask_access: S.ask_access === "full" ? "safe" : "full" });
  renderChrome();
  input.focus();
});
$("#ctx").addEventListener("change", async (e) => {
  if (e.target.id !== "proj") return;
  if (e.target.value === "__add__") {
    dialogOpen = true;
    const path = await invoke("pick_folder").catch(() => null);
    dialogOpen = false;
    if (path) {
      const name = path.split(/[\\/]/).filter(Boolean).pop() || path;
      const projects = [...(S.projects || []).filter((p) => p.path !== path), { name, path, profile: "" }];
      await saveSettings({ projects });
      conv.cwd = path;
    }
  } else conv.cwd = e.target.value;
  renderChrome();
  input.focus();
});

$("#thread").addEventListener("click", async (e) => {
  const link = e.target.closest("a[data-url]");
  if (link) {
    e.preventDefault();
    return invoke("plugin:opener|open_url", { url: link.dataset.url }).catch(() => {});
  }
  const cp = e.target.closest("pre .cp");
  if (cp) {
    await invoke("copy_text", { text: cp.nextElementSibling.textContent });
    cp.textContent = T.copied;
    return setTimeout(() => (cp.textContent = T.copy), 1200);
  }
  const b = e.target.closest("[data-act]");
  if (!b) return;
  const act = b.dataset.act;
  if (act === "more") {
    const k = Number(b.closest("[data-ans]")?.dataset.ans);
    openMenuFor = openMenuFor === k ? null : k;
    return render();
  }
  if (openMenuFor != null && act !== "more") openMenuFor = null;
  if (act === "img") return invoke("open_image", { path: b.dataset.path }).catch(() => {});
  if (act === "debate-more") return debateMore();
  if (act === "debate-sum") return debateSum();
  const a = findAnswer(b.closest("[data-ans]")?.dataset.ans);
  if (!a) return;
  if (act === "stop" && a.run) invoke("ask_cancel", { run: a.run });
  if (act === "relogin") {
    const t = tools.tools.find((x) => x.engine === a.engine);
    invoke("ask_login", { engine: a.engine, runtime: t?.runtime || "windows", profile: effectiveProfile().id }).catch((err) => alert(String(err)));
  }
  if (act === "insert") invoke("ask_insert", { text: a.text });
  if (["insert", "open-chatgpt", "open-claude"].includes(act)) render();
  if (act === "copy") {
    await invoke("copy_text", { text: a.text });
    b.textContent = T.copied;
    setTimeout(() => (b.textContent = T.copy), 1200);
  }
  if (act === "open-chatgpt" || act === "open-claude") {
    invoke("ask_open_in", { target: act === "open-chatgpt" ? "chatgpt" : "claude", text: questionFor(a) });
  }
  if (act === "steps") {
    a.showSteps = !a.showSteps;
    render();
  }
});

function fresh(focus) {
  if (conv?.items.length) persist(conv);
  const engine = S.ask_engine && available(S.ask_engine) ? S.ask_engine : firstAvailable() || "claude";
  conv = newConv();
  conv.engine = engine;
  selection = null;
  setExpanded(false);
  render();
  if (focus) input.focus();
}

listen("whiskr:ask-prefill", async (ev) => {
  const { text, send: go } = ev.payload || {};
  await loadSettings();
  if (!shown) {
    $("#shell").dataset.state = "compact";
    enterBar();
    updateRegion();
  }
  if (!conv || conv.items.length) fresh(false);
  input.value = text || "";
  autosize();
  input.focus();
  if (go) send();
});

// ───────── showing / hiding ─────────
// Hidden: reset quietly to the small, invisible bar — so the next Shift+Space
// starts clean: bar fades/grows in from the centre, then (if a chat continues)
// slides up into the full chat.
listen("whiskr:ask-hidden", () => {
  layoutSeq++;
  lastRegion = "";
  closeMenu();
  const panel = $("#panel");
  panel.classList.remove("anim", "enter", "leave");
  panel.style.height = "";
  panel.classList.add("away");
  dbg("ask-hidden");
  closing = false;
  shown = false;
  clearTimeout(closeTimer);
  $("#shell").dataset.state = "compact";
});

function enterBar() {
  const panel = $("#panel");
  dbg("enterBar (shown)");
  closing = false;
  clearTimeout(closeTimer); // a close still pending from before must not hide the new opening
  shown = true;
  dirty.clear();
  requestAnimationFrame(() => {
    render();
    kickTypewriter();
  });
  panel.classList.remove("away", "enter", "leave");
  void panel.offsetWidth; // restart the animation
  panel.classList.add("enter");
}

function continueChat(c) {
  if (c && c !== conv) {
    if (conv.items.length) persist(conv);
    conv = c;
  }
  render();
  // let the bar land first, then slide up
  setTimeout(() => setExpanded(true), 170);
}

listen("whiskr:ask-open", async (ev) => {
  const p = ev.payload || {};
  dbg(`ask-open phase=${p.phase} switch=${!!p.switch}`);
  if (p.phase === "show") {
    if (p.switch) {
      // Shift+Space+R right after opening: the other kind of chat
      if (p.fresh) {
        layoutSeq++;
        const panel = $("#panel");
        panel.classList.remove("anim");
        panel.style.height = "";
        fresh(false);
        input.value = "";
        autosize();
      } else {
        continueChat(conv.items.length ? conv : convs[0]);
      }
      return;
    }
    $("#shell").dataset.state = "compact";
    enterBar();
    updateRegion();
    const recent = conv && conv.items.length && Date.now() - conv.updated < 15 * 60e3;
    const busy = conv && [...runs.values()].some((a) => a.conv === conv.id);
    closeMenu();
    if (!EMBED) sideOpen = false;
    $("#shell").classList.toggle("side-open", sideOpen);
    if (p.fresh || (!recent && !busy)) {
      fresh(false);
      if (p.fresh) {
        input.value = "";
        autosize();
      }
    } else continueChat(conv);
    loadSettings().then(() => {
      renderChrome();
      renderSide();
    });
  } else if (p.phase === "focus") {
    focusAt = performance.now();
    selection = p.selection ? { text: p.selection, app: p.app || "" } : null;
    renderChrome();
    autosize();
    compactH = 0;
    fitCompact();
    input.focus();
    input.select();
  }
});
listen("whiskr:ask-blur", () => {
  // A started chat stays put when you click elsewhere; only the empty bar hides.
  setTimeout(() => {
    if (!shown || closing || pinned || dialogOpen || document.hasFocus()) return;
    if ($("#shell").dataset.state === "expanded" || conv.items.length) return;
    hide("blur");
  }, 150);
});
listen("whiskr:tools", (ev) => {
  tools.tools = ev.payload || [];
  renderChrome();
});

// keep the collapsed window exactly the size of the bar
new ResizeObserver(() => {
  if ($("#shell").dataset.state === "compact") fitCompact($("#engine-menu").hidden ? 0 : $("#engine-menu").offsetHeight + 10);
}).observe($("#composer"));

// ───────── boot ─────────
$("#cat-mini").innerHTML = cat({ body: "#2A2A2E", eyes: "#F2F0EA" });
(async () => {
  await loadSettings();
  await loadTools(false);
  await loadStore();
  // names of the chosen models (e.g. "GPT-6.1-Sol") for the picker label
  for (const k of Object.keys(S.ask_models || {})) if (S.ask_models[k] && available(k)) loadModels(k).then(renderChrome);
  $("#shell").classList.toggle("side-open", sideOpen);
  fresh(false);
  if (EMBED) {
    $("#panel").classList.remove("away");
    $("#shell").dataset.state = "expanded";
    $("#shell").classList.add("side-open");
    sideOpen = true;
    renderChrome();
    renderSide(); // fills the main window's sidebar with the chat list
    setTimeout(() => input.focus(), 150);
  }
  if (!TAURI && location.hash.includes("demo")) demo();
})();

// ───────── browser preview mock ─────────
function mockApi() {
  const handlers = {};
  const fire = (name, payload) => (handlers[name] || []).forEach((h) => h({ payload }));
  let settings = {
    ui_lang: "cs", ask_engine: "claude", ask_access: "safe", active_profile: "personal", ask_typewriter: true, ask_models: { claude: "opus" },
    profiles: [{ id: "personal", name: "Osobní", key: "" }, { id: "work", name: "Práce", key: "work" }],
    projects: [{ name: "iris", path: "C:\\Users\\user\\code\\iris", profile: "personal" }, { name: "acme-web", path: "C:\\Users\\user\\work\\acme", profile: "work" }],
  };
  let seq = 1;
  const answers = {
    claude: "Krátce: **ano, jde to**. Doporučuju:\n\n1. Použít `Promise.all` pro paralelní volání\n2. Ošetřit chyby zvlášť\n\n```js\nconst [a, b] = await Promise.all([fa(), fb()]);\n```\nTím ušetříš zhruba polovinu času.",
    codex: "Souhlasím s paralelizací. Jen bych přidal timeout:\n\n- `AbortController` s limitem 10 s\n- retry s exponenciálním čekáním",
    groq: "Ano — paralelně přes Promise.all.",
    cursor: "Hotovo, upravil jsem 2 soubory.",
  };
  return {
    invoke: async (cmd, args = {}) => {
      await new Promise((r) => setTimeout(r, 30));
      switch (cmd) {
        case "get_settings": return settings;
        case "save_settings": return (settings = args.settings);
        case "ask_tools": return { tools: [{ engine: "claude", runtime: "windows" }, { engine: "codex", runtime: "windows" }, { engine: "codex", runtime: "wsl" }], groq: true };
        case "ask_models":
          await new Promise((r) => setTimeout(r, 300));
          return { claude: [{ id: "fable", name: "Fable 5.1" }, { id: "opus", name: "Opus 5.5" }, { id: "sonnet", name: "Sonnet 5.5" }, { id: "haiku", name: "Haiku 5.5" }], codex: [{ id: "gpt-6.1-sol", name: "GPT-6.1-Sol" }, { id: "gpt-6-astra", name: "GPT-6-Astra" }, { id: "gpt-5.5", name: "GPT-5.5" }], groq: [{ id: "openai/gpt-oss-120b", name: "openai/gpt-oss-120b" }, { id: "llama-3.3-70b-versatile", name: "llama-3.3-70b-versatile" }] }[args.engine] || [];
        case "ask_store_get": {
          const mk = (title, mode, ago) => ({ id: "c" + ago, title, mode, engine: "claude", cwd: "", sessions: {}, updated: Date.now() - ago * 36e5, items: [{ type: "user", text: title }] });
          return JSON.stringify({ convs: [mk("Jak napojit e-maily Acme na chatbota", "ask", 1), mk("Claude × GPT: lokální AI pro Vojtu", "debate", 3), mk("Přidej README do iris", "delegate", 26), mk("Prompt pro onboarding v AI Laboratoři", "ask", 50), mk("Rozpočet Jana — kontrola", "ask", 200)] });
        }
        case "ask_run": {
          const run = seq++;
          const { engine, mode } = args.req;
          const text = answers[engine] || "OK";
          let i = 0;
          setTimeout(() => fire("whiskr:agent", { run, kind: "session", id: "s" + run }), 50);
          if (mode === "delegate") {
            ["📄 README.md", "▶ npm test", "✎ src/app.ts", "▶ npm test"].forEach((l, k) => setTimeout(() => fire("whiskr:agent", { run, kind: "step", label: l }), 200 + k * 250));
          }
          const tick = () => {
            if (i >= text.length) return fire("whiskr:agent", { run, kind: "done", text: "" });
            const n = 6 + Math.floor(Math.random() * 10);
            fire("whiskr:agent", { run, kind: "delta", text: text.slice(i, i + n) });
            i += n;
            setTimeout(tick, 35);
          };
          setTimeout(tick, mode === "delegate" ? 1300 : 300);
          return run;
        }
        default: return null;
      }
    },
    listen: async (name, h) => {
      (handlers[name] ||= []).push(h);
      return () => {};
    },
  };
}

function demo() {
  shown = true; // browser preview: the "window" is always on screen
  const mode = new URLSearchParams(location.hash.slice(1)).get("mode") || "ask";
  if (location.hash.includes("side=1")) setTimeout(toggleSide, 100);
  setTimeout(() => {
    if (mode !== "compact") {
      if (mode !== "ask") setMode(mode);
      if (mode === "delegate") conv.cwd = S.projects[0].path;
      selection = mode === "ask" ? { text: "async function load() { const a = await fa(); const b = await fb(); }", app: "Cursor" } : null;
      input.value = mode === "delegate" ? "Přidej do README sekci o instalaci a spusť testy" : "Dá se tohle zrychlit?";
      send();
    } else {
      selection = { text: "Dobrý den, posílám nabídku…", app: "Gmail — Outlook" };
      renderChrome();
    }
  }, 200);
}
