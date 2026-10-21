// Recorder ("záznamník"): record meetings/calls, list & search them, and work
// with one: audio player linked to the transcript, summary by template, mind map,
// "ask" chat with time citations, transcript editing, Word/PDF export.

const COLORS = ["var(--black)", "#E5484D", "#3B82F6", "#16A34A", "#D97706", "#8B5CF6", "#0891B2", "#DB2777"];
const RATES = [1, 1.25, 1.5, 2, 0.75];

export function initRecordings(ctx) {
  const { invoke, listen, toast, esc, $, $$, T, settings, save, go } = ctx;

  let recordings = [];
  let current = null; // full recording shown in detail
  let status = { recording: false, seconds: 0, has_system: false };
  let tickStart = 0;
  let timer = null;
  let voicesInfo = { model_ready: true, me: null, voices: [] };
  let templates = [];
  let tab = "summary";
  let editing = false;
  let marks = [];
  let audioFor = null; // recording id loaded in the player
  const chats = { all: [] }; // id -> [{q, a, pending, error}]

  // ───────── helpers ─────────
  function fmt(sec) {
    const s = Math.max(0, Math.floor(sec || 0));
    const h = Math.floor(s / 3600), m = Math.floor((s / 60) % 60), r = s % 60;
    const mm = String(m).padStart(2, "0"), ss = String(r).padStart(2, "0");
    return h ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
  }
  function parseTs(t) {
    const p = t.split(":").map(Number);
    return p.reduce((a, b) => a * 60 + b, 0);
  }
  const lang = () => (settings().ui_lang === "en" ? "en" : "cs");

  // ───────── record card ─────────
  function renderRecCard() {
    const on = status.recording;
    $("#rec-card").classList.toggle("recording", on);
    $("#rec-state").textContent = on ? `● ${T().rec_recording}` : T().rec_ready;
    $("#rec-btn").title = on ? T().rec_stop : T().rec_start;
    $("#nav-rec-dot").hidden = !on;
    $("#rec-system").checked = !!settings().rec_system_audio;
    $("#rec-system").disabled = on;
    $("#rec-hint").hidden = voicesInfo.model_ready;
    $("#rec-mark").hidden = !on;
    $("#rec-mark-n").textContent = on && marks.length ? marks.length : "";
    clearInterval(timer);
    if (on) {
      tickStart = performance.now() - status.seconds * 1000;
      const tick = () => ($("#rec-time").textContent = fmt((performance.now() - tickStart) / 1000));
      tick();
      timer = setInterval(tick, 500);
    } else {
      $("#rec-time").textContent = "00:00";
      $("#rec-meter").style.width = "0";
    }
  }

  $("#rec-btn").addEventListener("click", async () => {
    $("#rec-btn").disabled = true;
    try {
      if (status.recording) {
        await invoke("stop_meeting");
        status = { recording: false, seconds: 0, has_system: false };
      } else {
        status = await invoke("start_meeting", { withSystem: $("#rec-system").checked });
        marks = [];
        if ($("#rec-system").checked && !status.has_system) toast("Zvuk z počítače nejde nahrát — nahrávám jen mikrofon");
      }
    } catch (e) {
      toast(String(e));
    } finally {
      $("#rec-btn").disabled = false;
      renderRecCard();
      refreshList();
    }
  });
  $("#rec-mark").addEventListener("click", async () => {
    try {
      const t = await invoke("add_bookmark");
      if (typeof t === "number") marks.push(t);
      renderRecCard();
      toast(`★ ${fmt(t)}`);
    } catch (e) {
      toast(String(e));
    }
  });

  $("#rec-system").addEventListener("change", (e) => save({ rec_system_audio: e.target.checked }, true));

  listen("whiskr:meeting", (ev) => {
    status = ev.payload;
    if (status.recording && !status.seconds) marks = [];
    renderRecCard();
  });
  listen("whiskr:bookmarks", (ev) => {
    marks = ev.payload || [];
    renderRecCard();
  });
  listen("whiskr:rec-level", (ev) => {
    const pct = Math.min(100, Math.sqrt(ev.payload) * 260);
    $("#rec-meter").style.width = pct.toFixed(0) + "%";
  });
  listen("whiskr:goto", (ev) => {
    go(ev.payload);
    showList();
  });

  // ───────── list ─────────
  function statusChip(r) {
    if (r.status === "processing") {
      const stage = T()["stage_" + r.stage] || T().st_processing;
      return `<span class="status processing"><span class="spin"></span>${esc(stage)}</span>`;
    }
    if (r.status === "recording") return `<span class="status processing">● ${esc(T().rec_recording)}</span>`;
    if (r.status === "error") return `<span class="status error">${esc(T().st_error)}</span>`;
    return "";
  }

  function metaLine(r) {
    const d = new Date(r.created);
    const date = d.toLocaleDateString(lang(), { day: "numeric", month: "long" }) + " " + d.toLocaleTimeString(lang(), { hour: "2-digit", minute: "2-digit" });
    const min = Math.max(1, Math.round(r.seconds / 60));
    const len = min >= 60 ? `${Math.floor(min / 60)} h ${min % 60} min` : `${min} min`;
    return `${date} · ${len} · ${r.has_system ? T().rec_with_system : T().rec_mic_only}`;
  }

  function renderList() {
    if (searchQuery) return renderSearch();
    $("#rec-list-head").textContent = T().rec_list;
    const box = $("#rec-list");
    if (!recordings.length) {
      box.innerHTML = `<div class="card empty">${esc(T().rec_empty)}</div>`;
      return;
    }
    box.innerHTML =
      `<div class="card" style="padding:4px 6px">` +
      recordings
        .map((r) => {
          const topics = (r.summary?.topics || []).slice(0, 4).map((t) => `<span class="tag">${esc(t)}</span>`).join("");
          return `<div class="rec-item" data-id="${esc(r.id)}">
            <div style="min-width:0"><h3>${esc(r.title)}</h3><div class="m">${esc(metaLine(r))}</div>${topics ? `<div class="tags">${topics}</div>` : ""}</div>
            <div>${statusChip(r)}</div>
          </div>`;
        })
        .join("") +
      `</div>`;
  }

  async function refreshList() {
    recordings = await invoke("list_recordings").catch(() => []);
    renderList();
  }

  $("#rec-list").addEventListener("click", (e) => {
    const hit = e.target.closest(".hit");
    if (hit) return openDetail(hit.dataset.id, false, { seek: Number(hit.dataset.t), tab: "transcript" });
    const item = e.target.closest(".rec-item");
    if (item) openDetail(item.dataset.id);
  });

  listen("whiskr:recording", (ev) => {
    const r = ev.payload;
    const i = recordings.findIndex((x) => x.id === r.id);
    if (r.status === "done") loadVoices();
    if (i >= 0) recordings[i] = r;
    else recordings.unshift(r);
    renderList();
    if (current && current.id === r.id) openDetail(r.id, true);
  });

  // ───────── search ─────────
  let searchQuery = "";
  let searchHits = [];
  let searchTimer = null;
  function markTerms(text, q) {
    const words = q.toLowerCase().split(/[^\p{L}\p{N}]+/u).filter((w) => w.length >= 3).map((w) => w.slice(0, 5));
    let html = esc(text);
    for (const w of words) {
      const re = new RegExp(`(${w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}[\\p{L}\\p{N}]*)`, "giu");
      html = html.replace(re, "<mark>$1</mark>");
    }
    return html;
  }
  function renderSearch() {
    const t = T();
    $("#rec-list-head").textContent = `${t.rec_results}: ${searchHits.length}`;
    if (!searchHits.length) {
      $("#rec-list").innerHTML = `<div class="card empty">${esc(t.rec_no_results)}</div>`;
      return;
    }
    $("#rec-list").innerHTML =
      `<div class="card" style="padding:4px 6px">` +
      searchHits
        .map(
          (h) => `<div class="rec-item hit" data-id="${esc(h.id)}" data-t="${h.time}">
            <div style="min-width:0"><h3>${esc(h.title)} <span class="ts">${fmt(h.time)}</span></h3>
            <div class="snip"><b>${esc(h.speaker)}:</b> ${markTerms(h.snippet, searchQuery)}</div></div>
          </div>`
        )
        .join("") +
      `</div>`;
  }
  $("#rec-search").addEventListener("input", (e) => {
    clearTimeout(searchTimer);
    const q = e.target.value.trim();
    searchTimer = setTimeout(async () => {
      searchQuery = q;
      if (!q) return renderList();
      searchHits = await invoke("search_recordings", { query: q }).catch(() => []);
      if (searchQuery === q) renderSearch();
    }, 220);
  });

  // ───────── ask (chat) ─────────
  function citeHtml(id, ts, scope) {
    const t = parseTs(ts);
    const rec = recordings.find((r) => r.id === id);
    const label = scope === "all" && rec ? `${rec.title.length > 26 ? rec.title.slice(0, 25) + "…" : rec.title} · ${ts}` : ts;
    return `<button class="cite" data-id="${esc(id)}" data-t="${t}">▶ ${esc(label)}</button>`;
  }
  function answerHtml(text, scope) {
    // escape first, then light markdown + citations
    let h = esc(text);
    h = h.replace(/\[\[([\w-]+)@(\d{1,2}:\d{2}(?::\d{2})?)\]\]/g, (_, id, ts) => citeHtml(id, ts, scope));
    h = h.replace(/\*\*(.+?)\*\*/g, "<b>$1</b>");
    const lines = h.split("\n");
    let out = "", inList = false;
    for (const l of lines) {
      const m = l.match(/^\s*(?:[-*•]|\d+\.)\s+(.*)$/);
      if (m) {
        if (!inList) (out += "<ul>"), (inList = true);
        out += `<li>${m[1]}</li>`;
      } else {
        if (inList) (out += "</ul>"), (inList = false);
        if (l.trim()) out += `<p>${l}</p>`;
      }
    }
    if (inList) out += "</ul>";
    return out;
  }
  function renderChat(box) {
    const scope = box.dataset.scope;
    const key = scope === "all" ? "all" : current?.id;
    if (!key) return;
    const msgs = chats[key] || (chats[key] = []);
    const t = T();
    const sug = (scope === "all" ? t.ask_sug_all : t.ask_sug_one) || [];
    let html = `<div class="msgs">`;
    if (!msgs.length) {
      html += `<p class="muted small" style="margin:4px 0 10px">${esc(scope === "all" ? t.ask_hint_all : t.ask_hint_one)}</p>
        <div class="sugs">${sug.map((s) => `<button class="sug">${esc(s)}</button>`).join("")}</div>`;
    }
    for (const m of msgs) {
      html += `<div class="msg-q">${esc(m.q)}</div>`;
      if (m.pending) html += `<div class="msg-a pending"><span class="spin"></span>${esc(t.ask_thinking)}</div>`;
      else if (m.error) html += `<div class="msg-a err">${esc(m.error)}</div>`;
      else html += `<div class="msg-a">${answerHtml(m.a, scope)}</div>`;
    }
    html += `</div><form class="ask-form"><input class="input" name="q" placeholder="${esc(t.ask_placeholder)}" autocomplete="off" /><button class="btn btn-dark btn-sm">${esc(t.ask_send)}</button></form>`;
    const had = box.querySelector("input[name=q]");
    const draft = had ? had.value : "";
    const focused = had && document.activeElement === had;
    box.innerHTML = html;
    const inp = box.querySelector("input[name=q]");
    inp.value = draft;
    if (focused) inp.focus();
    const list = box.querySelector(".msgs");
    list.scrollTop = list.scrollHeight;
  }
  async function askQuestion(box, q) {
    q = q.trim();
    if (!q) return;
    const scope = box.dataset.scope;
    const key = scope === "all" ? "all" : current.id;
    const msgs = chats[key] || (chats[key] = []);
    const history = msgs.filter((m) => m.a).map((m) => [m.q, m.a]);
    const m = { q, pending: true };
    msgs.push(m);
    renderChat(box);
    try {
      m.a = await invoke("ask_recordings", { id: scope === "all" ? null : key, question: q, history });
    } catch (e) {
      m.error = String(e);
    }
    m.pending = false;
    if (scope === "all" || (current && current.id === key)) renderChat(box);
  }
  for (const box of $$(".chat")) {
    box.addEventListener("submit", (e) => {
      e.preventDefault();
      const inp = box.querySelector("input[name=q]");
      const q = inp.value;
      inp.value = "";
      askQuestion(box, q);
    });
    box.addEventListener("click", (e) => {
      const sug = e.target.closest(".sug");
      if (sug) return askQuestion(box, sug.textContent);
    });
  }
  $("#rec-ask-all-btn").addEventListener("click", () => {
    const box = $("#rec-ask-all");
    box.hidden = !box.hidden;
    if (!box.hidden) {
      renderChat($("#rec-ask-all .chat"));
      $("#rec-ask-all input[name=q]").focus();
    }
  });
  $("#rec-ask-all-close").addEventListener("click", () => ($("#rec-ask-all").hidden = true));

  // Citations and timestamps anywhere jump into the player.
  document.addEventListener("click", (e) => {
    const c = e.target.closest(".cite, .jump");
    if (!c || !c.closest("#page-recordings")) return;
    e.preventDefault();
    const t = Number(c.dataset.t);
    const id = c.dataset.id || current?.id;
    if (current && current.id === id) seek(t, true);
    else openDetail(id, false, { seek: t, play: true, tab: tab === "ask" ? "transcript" : tab });
  });

  // ───────── player ─────────
  const audio = $("#rec-audio");
  let rateIdx = 0;
  let nowIdx = -1;

  async function loadAudio() {
    const r = current;
    if (!r || r.status === "recording" || audioFor === r.id) return;
    audio.pause();
    audioFor = r.id;
    $("#rec-player").hidden = true;
    try {
      const path = await invoke("recording_audio", { id: r.id });
      if (!current || current.id !== r.id || !path) return;
      const conv = window.__TAURI__?.core?.convertFileSrc;
      audio.src = conv ? conv(path) : path;
      audio.playbackRate = RATES[rateIdx];
      $("#rec-player").hidden = false;
      renderMarks();
    } catch (e) {
      audioFor = null;
      console.warn("audio", e);
    }
  }
  function duration() {
    return isFinite(audio.duration) && audio.duration > 0 ? audio.duration : current?.seconds || 0;
  }
  function seek(t, play) {
    if ($("#rec-player").hidden) return;
    const go = () => {
      audio.currentTime = Math.max(0, t - 0.15);
      if (play) audio.play().catch((e) => toast(String(e)));
      updatePlayer(true);
    };
    if (audio.readyState >= 1) go();
    else audio.addEventListener("loadedmetadata", go, { once: true });
  }
  function renderMarks() {
    const d = duration();
    const bm = current?.bookmarks || [];
    $("#pl-marks").innerHTML = d ? bm.map((t) => `<i style="left:${((t / d) * 100).toFixed(2)}%" title="★ ${fmt(t)}"></i>`).join("") : "";
  }
  function updatePlayer(scroll) {
    const d = duration();
    const t = audio.currentTime || 0;
    $("#pl-fill").style.width = d ? ((t / d) * 100).toFixed(2) + "%" : "0";
    $("#pl-time").textContent = `${fmt(t)} / ${fmt(d)}`;
    $("#rec-player").classList.toggle("playing", !audio.paused);
    // highlight current transcript line
    const us = current?.utterances || [];
    let idx = -1;
    for (let i = 0; i < us.length; i++) {
      if (us[i].start <= t + 0.3) idx = i;
      else break;
    }
    if (idx !== nowIdx) {
      $$("#rec-detail .utt.now").forEach((el) => el.classList.remove("now"));
      nowIdx = idx;
      const el = idx >= 0 ? $(`#rec-detail .utt[data-i="${idx}"]`) : null;
      if (el) {
        el.classList.add("now");
        if ((scroll || !audio.paused) && !editing) el.scrollIntoView({ block: "nearest", behavior: "smooth" });
      }
    }
  }
  audio.addEventListener("timeupdate", () => updatePlayer(false));
  audio.addEventListener("play", () => updatePlayer(false));
  audio.addEventListener("pause", () => updatePlayer(false));
  audio.addEventListener("loadedmetadata", () => (renderMarks(), updatePlayer(false)));
  $("#pl-play").addEventListener("click", () => (audio.paused ? audio.play().catch((e) => toast(String(e))) : audio.pause()));
  $("#pl-back").addEventListener("click", () => seek(Math.max(0, audio.currentTime - 10), !audio.paused));
  $("#pl-rate").addEventListener("click", () => {
    rateIdx = (rateIdx + 1) % RATES.length;
    audio.playbackRate = RATES[rateIdx];
    $("#pl-rate").textContent = RATES[rateIdx] + "×";
  });
  $("#pl-track").addEventListener("click", (e) => {
    const r = e.currentTarget.getBoundingClientRect();
    seek(((e.clientX - r.left) / r.width) * duration(), !audio.paused);
  });
  document.addEventListener("keydown", (e) => {
    if (!current || $("#rec-detail-view").hidden || $("#rec-player").hidden) return;
    if (e.target.closest("input, textarea, select, [contenteditable=true]")) return;
    if (e.code === "Space") {
      e.preventDefault();
      $("#pl-play").click();
    }
  });

  // ───────── detail ─────────
  function showList() {
    current = null;
    editing = false;
    audio.pause();
    $("#rec-detail-view").hidden = true;
    $("#rec-list-view").hidden = false;
  }
  $("#rec-back").addEventListener("click", showList);

  function speakerOrder() {
    const order = [];
    for (const u of current.utterances || []) if (u.speaker !== "ME" && !order.includes(u.speaker)) order.push(u.speaker);
    for (const l of Object.keys(current.speakers || {})) if (l !== "ME" && l !== "OTHERS" && !order.includes(l)) order.push(l);
    return order;
  }
  function speakerColor(label, order) {
    if (label === "ME") return COLORS[0];
    const i = order.indexOf(label);
    return COLORS[1 + (i % (COLORS.length - 1))];
  }
  function nameOf(label) {
    return current.speakers?.[label] || (label === "ME" ? (lang() === "en" ? "Me" : "Já") : label === "OTHERS" ? (lang() === "en" ? "Others" : "Ostatní") : `${lang() === "en" ? "Speaker" : "Mluvčí"} ${label}`);
  }
  function tplName(id) {
    if (!id || id === "auto") return T().tpl_auto;
    return templates.find((t) => t.id === id)?.name || id;
  }

  function summaryHtml(r) {
    const t = T();
    const s = r.summary;
    let html = "";
    if (r.utterances?.length && r.status !== "processing") {
      html += `<div class="tpl-row">
        <span class="muted small">${esc(t.tpl_label)}</span>
        <select class="input input-sm" id="rec-tpl">
          <option value="auto">${esc(t.tpl_auto)}</option>
          ${templates.map((x) => `<option value="${esc(x.id)}">${esc(x.name)}</option>`).join("")}
        </select>
        <button class="btn btn-light btn-sm" id="rec-resum">${esc(t.tpl_redo)}</button>
        ${s?.template ? `<span class="muted small" style="margin-left:auto">${esc(t.tpl_used)}: <b>${esc(tplName(s.template))}</b></span>` : ""}
      </div>`;
    }
    if (!s) return html;
    html += `<div class="card summary">`;
    if (s.overview) html += `<h2>${esc(t.rec_summary)}</h2><p>${esc(s.overview)}</p>`;
    if (s.highlights?.length)
      html += `<h2 class="hl-head">${esc(t.rec_highlights)}</h2><ul class="highlights">${s.highlights
        .map((h) => `<li><button class="jump ts" data-t="${h.time}">${fmt(h.time)}</button> ${esc(h.text)}</li>`)
        .join("")}</ul>`;
    if (s.action_items?.length)
      html += `<h2>${esc(t.rec_tasks)}</h2><ul class="tasks">${s.action_items
        .map((a, i) => `<li>${esc(a)} <button class="to-task" data-task="${i}" title="${esc(t.rec_to_task)}">${esc(t.rec_to_task)}</button></li>`)
        .join("")}</ul>`;
    if (s.decisions?.length) html += `<h2>${esc(t.rec_decisions)}</h2><ul class="decisions">${s.decisions.map((a) => `<li>${esc(a)}</li>`).join("")}</ul>`;
    for (const sec of s.sections || []) {
      if (!sec.items?.length) continue;
      html += `<h2>${esc(sec.title)}</h2><ul>${sec.items.map((a) => `<li>${esc(a)}</li>`).join("")}</ul>`;
    }
    if (s.topics?.length) html += `<div class="tags" style="margin-top:16px">${s.topics.map((x) => `<span class="tag">${esc(x)}</span>`).join("")}</div>`;
    html += `</div>`;
    return html;
  }

  function transcriptHtml(r) {
    const t = T();
    if (!r.utterances?.length) return r.status === "done" ? `<div class="card empty">${esc(t.rec_no_speech)}</div>` : "";
    const order = speakerOrder();
    const labels = [...(r.utterances.some((u) => u.speaker === "ME") || r.speakers?.ME ? ["ME"] : []), ...order];
    const bm = r.bookmarks || [];
    const marked = new Set();
    for (const b of bm) {
      let k = -1;
      r.utterances.forEach((u, i) => u.start <= b + 0.5 && (k = i));
      if (k >= 0) marked.add(k);
    }
    let html = `<div class="section-head tr-head"><span class="muted small">${esc(editing ? t.tr_edit_hint : t.tr_play_hint)}</span>
      <button class="btn ${editing ? "btn-dark" : "btn-light"} btn-sm" id="tr-edit">${esc(editing ? t.tr_done : t.tr_edit)}</button></div>`;
    html += `<div class="card transcript${editing ? " editing" : ""}">`;
    html += r.utterances
      .map((u, i) => {
        const col = speakerColor(u.speaker, order);
        const who = editing
          ? `<select class="who" data-i="${i}" style="color:${col}">${labels
              .map((l) => `<option value="${esc(l)}"${l === u.speaker ? " selected" : ""}>${esc(nameOf(l))}</option>`)
              .join("")}<option value="__new__">${esc(t.tr_new_speaker)}</option></select>`
          : `<b style="color:${col}">${esc(nameOf(u.speaker))}${marked.has(i) ? ` <span class="star">★</span>` : ""}</b>`;
        return `<div class="utt${marked.has(i) ? " marked" : ""}" data-i="${i}">
          <button class="jump ts" data-t="${u.start}">${fmt(u.start)}</button>
          <div>${who}${editing && marked.has(i) ? ` <span class="star">★</span>` : ""}<div class="tx"${editing ? ` contenteditable="true" spellcheck="true"` : ""}>${esc(u.text)}</div></div>
        </div>`;
      })
      .join("");
    html += `</div>`;
    return html;
  }

  // Mind map: a left-to-right tree drawn as SVG.
  function wrap(text, max) {
    const words = String(text).split(/\s+/);
    const lines = [];
    let cur = "";
    for (const w of words) {
      if ((cur + " " + w).trim().length > max && cur) {
        lines.push(cur);
        cur = w;
      } else cur = (cur + " " + w).trim();
      if (lines.length === 2) break;
    }
    if (lines.length < 2 && cur) lines.push(cur);
    if (lines.length === 2 && words.join(" ").length > lines.join(" ").length + 1) lines[1] = lines[1].replace(/[,.;:]?$/, "…");
    return lines;
  }
  function mapHtml(r) {
    const s = r.summary;
    const t = T();
    if (!s) return `<div class="card empty">${esc(t.map_empty)}</div>`;
    const branches = [];
    if (s.action_items?.length) branches.push([t.rec_tasks, s.action_items]);
    if (s.decisions?.length) branches.push([t.rec_decisions, s.decisions]);
    for (const sec of s.sections || []) if (sec.items?.length) branches.push([sec.title, sec.items]);
    if (s.highlights?.length) branches.push([t.rec_highlights, s.highlights.map((h) => `${fmt(h.time)} ${h.text}`)]);
    if (!branches.length && s.topics?.length) branches.push([t.rec_topics, s.topics]);
    if (!branches.length) return `<div class="card empty">${esc(t.map_empty)}</div>`;

    const LEAF_W = 290, X0 = 12, ROOT_W = 170, BX = X0 + ROOT_W + 36, BR_W = 160, LX = BX + BR_W + 30;
    const W = LX + LEAF_W + 12;
    let y = 16;
    const nodes = [];
    branches.forEach(([title, items], bi) => {
      const color = COLORS[1 + (bi % (COLORS.length - 1))];
      const leaves = items.slice(0, 8).map((it) => {
        const lines = wrap(it, 40);
        const h = lines.length * 16 + 12;
        const leaf = { lines, y, h };
        y += h + 8;
        return leaf;
      });
      const top = leaves[0].y, bottom = leaves[leaves.length - 1].y + leaves[leaves.length - 1].h;
      nodes.push({ title, color, leaves, cy: (top + bottom) / 2 });
      y += 14;
    });
    const H = Math.max(y, 120);
    const rootLines = wrap(r.title, 20);
    const rootH = rootLines.length * 18 + 20;
    const rcy = H / 2;
    let svg = `<svg class="mindmap" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" xmlns="http://www.w3.org/2000/svg">`;
    for (const n of nodes) {
      svg += `<path d="M${X0 + ROOT_W} ${rcy} C ${X0 + ROOT_W + 30} ${rcy}, ${BX - 30} ${n.cy}, ${BX} ${n.cy}" stroke="${n.color}" stroke-width="2.2" fill="none" opacity=".55"/>`;
      for (const l of n.leaves) {
        const ly = l.y + l.h / 2;
        svg += `<path d="M${BX + BR_W} ${n.cy} C ${BX + BR_W + 22} ${n.cy}, ${LX - 22} ${ly}, ${LX} ${ly}" stroke="${n.color}" stroke-width="1.5" fill="none" opacity=".45"/>`;
      }
    }
    svg += `<rect x="${X0}" y="${rcy - rootH / 2}" width="${ROOT_W}" height="${rootH}" rx="16" fill="#111111"/>`;
    rootLines.forEach((ln, i) => {
      svg += `<text x="${X0 + ROOT_W / 2}" y="${rcy - rootH / 2 + 24 + i * 18}" text-anchor="middle" class="mm-root">${esc(ln)}</text>`;
    });
    for (const n of nodes) {
      const bl = wrap(n.title, 20);
      const bh = bl.length * 16 + 16;
      svg += `<rect x="${BX}" y="${n.cy - bh / 2}" width="${BR_W}" height="${bh}" rx="12" fill="${n.color}"/>`;
      bl.forEach((ln, i) => (svg += `<text x="${BX + BR_W / 2}" y="${n.cy - bh / 2 + 20 + i * 16}" text-anchor="middle" class="mm-branch">${esc(ln)}</text>`));
      for (const l of n.leaves) {
        svg += `<rect x="${LX}" y="${l.y}" width="${LEAF_W}" height="${l.h}" rx="9" class="mm-leaf" style="stroke:${n.color}"/>`;
        l.lines.forEach((ln, i) => (svg += `<text x="${LX + 12}" y="${l.y + 18 + i * 16}" class="mm-text">${esc(ln)}</text>`));
      }
    }
    svg += `</svg>`;
    return `<div class="card map-card"><div class="map-scroll">${svg}</div></div>`;
  }

  function renderDetail() {
    const r = current;
    const t = T();
    $("#rec-title").value = r.title;
    const engine = r.engine ? " · " + (t["rec_engine_" + r.engine] || r.engine) : "";
    $("#rec-meta").textContent = metaLine(r) + engine;
    $$("#rec-tabs .tab").forEach((b) => b.classList.toggle("on", b.dataset.tab === tab));
    const hasContent = r.utterances?.length;
    $("#rec-tabs").hidden = !hasContent;

    let html = "";
    if (r.status === "processing") {
      const stage = t["stage_" + r.stage] || t.st_processing;
      html += `<div class="card processing-box"><span class="spin"></span>${esc(stage)}</div>`;
    }
    if (r.status === "error" || r.error) html += `<div class="error-box">${esc(r.error || t.st_error)}</div>`;

    if (hasContent && tab === "summary") {
      const order = speakerOrder();
      const labels = [...(r.utterances.some((u) => u.speaker === "ME") ? ["ME"] : []), ...order.filter((l) => r.utterances.some((u) => u.speaker === l))];
      html += `<div class="section-head" style="margin-top:6px"><h2>${esc(t.rec_speakers)}</h2><span class="muted small">${esc(t.rec_rename_hint)}</span></div>
        <div class="speakers">${labels
          .map((l) => `<label class="speaker"><i style="background:${speakerColor(l, order)}"></i><input data-label="${esc(l)}" value="${esc(nameOf(l))}" spellcheck="false" /></label>`)
          .join("")}</div>`;
      html += summaryHtml(r);
    } else if (hasContent && tab === "transcript") html += transcriptHtml(r);
    else if (hasContent && tab === "map") html += mapHtml(r);
    else if (!hasContent && r.status === "done") html += `<div class="card empty" style="margin-top:18px">${esc(t.rec_no_speech)}</div>`;

    $("#rec-detail").innerHTML = html;
    const sel = $("#rec-tpl");
    if (sel) sel.value = r.template && templates.some((x) => x.id === r.template) ? r.template : "auto";

    const chat = $("#rec-chat");
    chat.hidden = !(hasContent && tab === "ask");
    if (!chat.hidden) renderChat(chat);
    nowIdx = -1;
    updatePlayer(false);
  }

  async function openDetail(id, quiet = false, opts = {}) {
    try {
      current = await invoke("get_recording", { id });
    } catch (e) {
      if (!quiet) toast(String(e));
      return;
    }
    if (!current) return;
    if (!quiet) {
      editing = false;
      if (opts.tab) tab = opts.tab;
      else if (audioFor !== id) tab = "summary";
    }
    $("#rec-list-view").hidden = true;
    $("#rec-detail-view").hidden = false;
    renderDetail();
    if (!quiet) $(".main").scrollTop = 0;
    await loadAudio();
    if (opts.seek != null) {
      seek(opts.seek, !!opts.play);
      setTimeout(() => $(`#rec-detail .utt.now`)?.scrollIntoView({ block: "center" }), 120);
    }
  }

  $("#rec-tabs").addEventListener("click", (e) => {
    const b = e.target.closest(".tab");
    if (!b || !current) return;
    tab = b.dataset.tab;
    renderDetail();
    if (tab === "ask") $("#rec-chat input[name=q]")?.focus();
  });

  // click a sentence → play from there
  $("#rec-detail").addEventListener("click", async (e) => {
    if (!current) return;
    const tt = e.target.closest(".to-task");
    if (tt) {
      const text = current.summary?.action_items?.[Number(tt.dataset.task)];
      if (!text) return;
      try {
        await invoke("add_task", { text, smart: true, source: "meeting", sourceId: current.id });
        tt.textContent = "✓";
        tt.disabled = true;
        toast(T().rec_to_task_ok);
      } catch (err) {
        toast(String(err));
      }
      return;
    }
    if (e.target.closest("#tr-edit")) {
      editing = !editing;
      return renderDetail();
    }
    if (e.target.closest("#rec-resum")) {
      try {
        await invoke("resummarize", { id: current.id, template: $("#rec-tpl").value });
        openDetail(current.id, true);
      } catch (err) {
        toast(String(err));
      }
      return;
    }
    if (editing) return;
    const utt = e.target.closest(".utt");
    if (utt && !e.target.closest(".jump") && !window.getSelection()?.toString()) {
      const u = current.utterances[Number(utt.dataset.i)];
      if (u) seek(u.start, true);
    }
  });

  // speaker rename (summary tab) + reassign (edit mode)
  $("#rec-detail").addEventListener("change", async (e) => {
    if (!current) return;
    const who = e.target.closest("select.who");
    if (who) {
      try {
        current = await invoke("update_utterance", { id: current.id, index: Number(who.dataset.i), text: null, speaker: who.value });
        renderDetail();
      } catch (err) {
        toast(String(err));
      }
      return;
    }
    const inp = e.target.closest(".speaker input");
    if (!inp) return;
    try {
      current = await invoke("rename_speaker", { id: current.id, label: inp.dataset.label, name: inp.value });
      renderDetail();
      loadVoices();
    } catch (err) {
      toast(String(err));
    }
  });
  // text edits are saved when leaving the line
  $("#rec-detail").addEventListener("focusout", async (e) => {
    const tx = e.target.closest?.(".tx[contenteditable=true]");
    if (!tx || !current) return;
    const i = Number(tx.closest(".utt").dataset.i);
    const text = tx.innerText.replace(/\s+/g, " ").trim();
    if (text === current.utterances[i]?.text) return;
    try {
      current = await invoke("update_utterance", { id: current.id, index: i, text, speaker: null });
      if (!text) renderDetail();
    } catch (err) {
      toast(String(err));
    }
  });
  $("#rec-detail").addEventListener("keydown", (e) => {
    if (e.key === "Enter" && e.target.matches(".speaker input, .tx[contenteditable=true]")) {
      e.preventDefault();
      e.target.blur();
    }
  });

  $("#rec-title").addEventListener("change", async (e) => {
    if (!current) return;
    await invoke("rename_recording", { id: current.id, title: e.target.value }).catch((err) => toast(String(err)));
    current.title = e.target.value;
    refreshList();
  });
  $("#rec-title").addEventListener("keydown", (e) => e.key === "Enter" && e.target.blur());

  $("#rec-copy").addEventListener("click", async () => {
    const md = await invoke("recording_markdown", { id: current.id }).catch((e) => (toast(String(e)), null));
    if (md == null) return;
    await invoke("copy_text", { text: md });
    toast(T().rec_copied);
  });
  $("#rec-word").addEventListener("click", async () => {
    try {
      await invoke("export_docx", { id: current.id });
      toast(T().exported);
    } catch (e) {
      toast(String(e));
    }
  });
  $("#rec-pdf").addEventListener("click", () => {
    const r = current;
    const t = T();
    const order = speakerOrder();
    const saveTab = tab;
    let html = `<h1>${esc(r.title)}</h1><div class="pm">${esc(metaLine(r))}</div>`;
    html += summaryHtml(r).replace(/<div class="tpl-row">[\s\S]*?<\/div>/, "");
    if (r.utterances?.length)
      html += `<h2 class="ph">${esc(t.rec_transcript)}</h2>` + r.utterances
        .map((u) => `<div class="pu"><span class="pt">${fmt(u.start)}</span> <b style="color:${speakerColor(u.speaker, order)}">${esc(nameOf(u.speaker))}:</b> ${esc(u.text)}</div>`)
        .join("");
    $("#rec-print").innerHTML = html;
    const theme = document.documentElement.dataset.theme;
    document.documentElement.dataset.theme = "light";
    document.body.classList.add("printing");
    const done = () => {
      document.documentElement.dataset.theme = theme;
      document.body.classList.remove("printing");
      $("#rec-print").innerHTML = "";
      tab = saveTab;
    };
    window.addEventListener("afterprint", done, { once: true });
    setTimeout(() => {
      window.print();
      setTimeout(done, 1500);
    }, 50);
  });
  $("#rec-folder").addEventListener("click", () => invoke("open_recording_folder", { id: current.id }).catch((e) => toast(String(e))));
  $("#rec-redo").addEventListener("click", async () => {
    audio.pause();
    audioFor = null;
    await invoke("reprocess_recording", { id: current.id }).catch((e) => toast(String(e)));
    openDetail(current.id, true);
  });
  $("#rec-delete").addEventListener("click", async () => {
    if (!confirm(T().rec_delete_confirm)) return;
    audio.pause();
    audio.removeAttribute("src");
    audioFor = null;
    await invoke("delete_recording", { id: current.id }).catch((e) => toast(String(e)));
    showList();
    refreshList();
  });

  // ───────── settings: templates ─────────
  async function loadTemplates() {
    templates = await invoke("list_templates").catch(() => templates);
    renderTemplateSettings();
  }
  function renderTemplateSettings() {
    const t = T();
    const sel = $("#set-template");
    sel.innerHTML = `<option value="auto">${esc(t.tpl_auto)}</option>` + templates.map((x) => `<option value="${esc(x.id)}">${esc(x.name)}</option>`).join("");
    sel.value = settings().default_template || "auto";
    if (sel.value === "") sel.value = "auto";
    const custom = settings().custom_templates || [];
    $("#tpl-list").innerHTML = custom
      .map(
        (c, i) => `<div class="tpl-item" data-i="${i}">
          <input class="input input-sm" data-k="name" value="${esc(c.name)}" placeholder="${esc(t.tpl_name)}" />
          <textarea class="input" data-k="instructions" rows="2" placeholder="${esc(t.tpl_instr)}">${esc(c.instructions)}</textarea>
          <button class="btn btn-ghost btn-sm" data-del="${i}">${esc(t.tpl_delete)}</button>
        </div>`
      )
      .join("");
  }
  async function saveCustom(list) {
    if (await save({ custom_templates: list }, true)) await loadTemplates();
  }
  $("#set-template").addEventListener("change", (e) => save({ default_template: e.target.value }));
  $("#tpl-add").addEventListener("click", async () => {
    const list = [...(settings().custom_templates || [])];
    list.push({ id: "custom-" + Date.now().toString(36), name: lang() === "en" ? "My template" : "Moje šablona", description: "", instructions: "" });
    await saveCustom(list);
    $("#tpl-list .tpl-item:last-child input")?.select();
  });
  $("#tpl-list").addEventListener("click", async (e) => {
    const b = e.target.closest("[data-del]");
    if (!b) return;
    const list = (settings().custom_templates || []).filter((_, i) => i !== Number(b.dataset.del));
    await saveCustom(list);
  });
  $("#tpl-list").addEventListener("change", async (e) => {
    const f = e.target.closest("[data-k]");
    if (!f) return;
    const i = Number(f.closest(".tpl-item").dataset.i);
    const list = (settings().custom_templates || []).map((c) => ({ ...c }));
    list[i][f.dataset.k] = f.value;
    if (f.dataset.k === "instructions") list[i].description = f.value.slice(0, 80);
    await saveCustom(list);
  });

  // ───────── settings: known voices + MCP ─────────
  async function loadVoices() {
    voicesInfo = await invoke("list_voices").catch(() => voicesInfo);
    renderVoices();
    renderRecCard();
  }
  function renderVoices() {
    const t = T();
    const me = voicesInfo.me
      ? `<span class="chip">🎙️ ${esc(t.voices_me)} <span class="muted small">· ${esc(t.voices_me_n.replace("{n}", voicesInfo.me))}</span><button data-forget="Já" title="${esc(t.voices_forget)}">✕</button></span>`
      : `<span class="chip">🎙️ ${esc(t.voices_me)} <span class="muted small">· ${esc(t.voices_me_none)}</span></span>`;
    const others = voicesInfo.voices.length
      ? voicesInfo.voices.map((v) => `<span class="chip">${esc(v.name)}<button data-forget="${esc(v.name)}" title="${esc(t.voices_forget)}">✕</button></span>`).join("")
      : `<span class="muted small" style="align-self:center">${esc(t.voices_empty)}</span>`;
    $("#voice-chips").innerHTML = me + others;
    $("#voice-model").textContent = voicesInfo.model_ready ? t.voices_model_ok : t.voices_model_no;
  }
  $("#voice-chips").addEventListener("click", async (e) => {
    const b = e.target.closest("button[data-forget]");
    if (!b) return;
    if (!confirm(T().voices_forget_confirm.replace("{name}", b.dataset.forget))) return;
    await invoke("forget_voice", { name: b.dataset.forget }).catch((err) => toast(String(err)));
    loadVoices();
  });

  // ───────── boot ─────────
  return {
    async init() {
      status = await invoke("meeting_status").catch(() => status);
      await loadVoices();
      await loadTemplates();
      renderRecCard();
      await refreshList();
    },
    rerender() {
      renderRecCard();
      renderList();
      renderVoices();
      renderTemplateSettings();
      if (current) renderDetail();
      if (!$("#rec-ask-all").hidden) renderChat($("#rec-ask-all .chat"));
    },
  };
}
