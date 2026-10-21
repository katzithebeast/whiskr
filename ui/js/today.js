// "Dnes": morning briefing, tasks & reminders, brief before a meeting.
import { md } from "./md.js";
import { keyLabels } from "./cat.js";

export function initToday(ctx) {
  const { invoke, listen, toast, esc, $, T, settings } = ctx;
  let tasks = [];
  let briefing = null;
  let busy = false;
  let editing = null; // task id whose due is being edited
  let showDone = false;

  const loc = () => (settings().ui_lang === "en" ? "en" : "cs");
  const fill = (s, o) => s.replace(/\{(\w+)\}/g, (_, k) => o[k] ?? "");
  const dayKey = (d) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  const taskKeys = () => keyLabels(settings().hotkey_task || "").join("+");

  function whenLabel(t) {
    if (!t.due) return "";
    const d = new Date(t.due);
    const now = new Date();
    const days = Math.round((new Date(d.toDateString()) - new Date(now.toDateString())) / 864e5);
    const words = loc() === "en" ? ["today", "tomorrow", "yesterday"] : ["dnes", "zítra", "včera"];
    let day =
      days === 0 ? words[0] : days === 1 ? words[1] : days === -1 ? words[2] : days > 1 && days < 7 ? d.toLocaleDateString(loc(), { weekday: "long" }) : d.toLocaleDateString(loc(), { day: "numeric", month: "numeric" });
    if (t.has_time) day += " " + d.toLocaleTimeString(loc(), { hour: "numeric", minute: "2-digit" });
    return day;
  }

  function group(t) {
    if (t.done) return "done";
    if (!t.due) return "none";
    const d = new Date(t.due);
    const now = new Date();
    if (d < now && (t.has_time || dayKey(d) < dayKey(now))) return "late";
    if (dayKey(d) === dayKey(now)) return "today";
    return "later";
  }

  function localInput(due) {
    const d = due ? new Date(due) : new Date(Date.now() + 36e5);
    const p = (n) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
  }

  function row(t) {
    const t_ = T();
    const g = group(t);
    const src = t.source === "meeting" ? `<span class="t-src" title="${esc(t_.t_from_rec)}">📼</span>` : t.source === "voice" ? `<span class="t-src" title="${esc(t.said || t_.t_voice)}">🎙</span>` : "";
    const due =
      editing === t.id
        ? `<input type="datetime-local" class="input input-sm t-due-edit" value="${localInput(t.due)}" /><button class="btn btn-ghost btn-sm" data-act="clear-due">${esc(t_.t_due_clear)}</button>`
        : t.due
        ? `<button class="t-due ${g}" data-act="edit-due">${t.has_time ? "⏰ " : ""}${esc(whenLabel(t))}</button>`
        : t.done
        ? ""
        : `<button class="t-due add" data-act="edit-due">＋ ${esc(t_.t_due_set)}</button>`;
    const acts = t.done
      ? `<button class="icon-btn" data-act="del" title="${esc(t_.t_del)}">✕</button>`
      : `<button class="btn btn-ghost btn-sm" data-act="snooze-h">${esc(t_.t_snooze_h)}</button><button class="btn btn-ghost btn-sm" data-act="snooze-d">${esc(t_.t_snooze_d)}</button><button class="icon-btn" data-act="del" title="${esc(t_.t_del)}">✕</button>`;
    return `<div class="task ${t.done ? "done" : ""}" data-id="${esc(t.id)}">
      <label class="t-check"><input type="checkbox" ${t.done ? "checked" : ""} data-act="toggle" /><i></i></label>
      <div class="t-text" data-act="edit-text" title="${esc(T().t_edit)}">${esc(t.text)}</div>
      ${src}<div class="t-due-box">${due}</div><div class="t-acts">${acts}</div>
    </div>`;
  }

  function renderTasks() {
    const t_ = T();
    const groups = { late: [], today: [], later: [], none: [], done: [] };
    for (const t of tasks) groups[group(t)].push(t);
    const byDue = (a, b) => new Date(a.due) - new Date(b.due);
    groups.late.sort(byDue);
    groups.today.sort(byDue);
    groups.later.sort(byDue);
    groups.done.sort((a, b) => new Date(b.done_at || b.created) - new Date(a.done_at || a.created));
    const weekAgo = Date.now() - 7 * 864e5;
    groups.done = groups.done.filter((t) => new Date(t.done_at || t.created) > weekAgo);
    let html = "";
    const open = groups.late.length + groups.today.length + groups.later.length + groups.none.length;
    if (!open && !groups.done.length) html = `<div class="empty muted">${esc(fill(t_.tasks_empty, { k: taskKeys() }))}</div>`;
    for (const k of ["late", "today", "later", "none"]) {
      if (!groups[k].length) continue;
      html += `<div class="t-group ${k}"><div class="t-gh">${esc(t_["tg_" + k])} <span>${groups[k].length}</span></div>${groups[k].map(row).join("")}</div>`;
    }
    if (groups.done.length) {
      html += `<div class="t-group done"><button class="t-gh link" data-act="show-done">${esc(t_.tg_done)} <span>${groups.done.length}</span> ${showDone ? "▾" : "▸"}</button>${showDone ? groups.done.map(row).join("") : ""}</div>`;
    }
    $("#task-list").innerHTML = html;
    $("#task-hint").textContent = fill(t_.task_hint, { k: taskKeys() });
    const badge = groups.late.length + groups.today.length;
    $("#nav-today-badge").hidden = !badge;
    $("#nav-today-badge").textContent = badge;
    $("#nav-today-badge").classList.toggle("late", groups.late.length > 0);
  }

  function renderBriefing() {
    const t_ = T();
    const today = dayKey(new Date());
    $("#today-date").textContent = new Date().toLocaleDateString(loc(), { weekday: "long", day: "numeric", month: "long" });
    const body = $("#briefing-body");
    $("#briefing-refresh").textContent = briefing ? t_.briefing_refresh : t_.briefing_now;
    $("#briefing-refresh").disabled = busy;
    if (busy) {
      body.innerHTML = `<div class="wait"><span class="spin"></span>${esc(t_.briefing_busy)}</div>` + (briefing ? `<div class="md faded">${md(briefing.text)}</div>` : "");
      $("#briefing-meta").textContent = "";
      return;
    }
    if (!briefing) {
      body.innerHTML = `<p class="muted">${esc(fill(t_.briefing_empty, { t: settings().briefing_time || "08:00" }))}</p>`;
      $("#briefing-meta").textContent = "";
      return;
    }
    const gen = briefing.generated ? new Date(briefing.generated) : null;
    const parts = [];
    if (gen) parts.push(gen.toLocaleTimeString(loc(), { hour: "numeric", minute: "2-digit" }));
    if (briefing.engine) parts.push(fill(t_.briefing_by, { e: briefing.engine }));
    $("#briefing-meta").textContent = parts.join(" · ");
    let html = "";
    if (briefing.date !== today) html += `<p class="muted small">${esc(fill(t_.briefing_old, { d: gen ? gen.toLocaleDateString(loc()) : briefing.date }))}</p>`;
    if (briefing.error) html += `<p class="warn small" title="${esc(briefing.error)}">⚠ ${esc(t_.briefing_err)}: ${esc(briefing.error.slice(0, 160))}</p>`;
    html += md(briefing.text);
    body.innerHTML = html;
  }

  function render() {
    renderTasks();
    renderBriefing();
  }

  async function update(t) {
    await invoke("update_task", { task: t });
  }

  // ───────── events ─────────
  $("#task-list").addEventListener("click", async (e) => {
    const el = e.target.closest("[data-act]");
    if (!el) return;
    const act = el.dataset.act;
    if (act === "show-done") {
      showDone = !showDone;
      return renderTasks();
    }
    const id = el.closest("[data-id]")?.dataset.id;
    const t = tasks.find((x) => x.id === id);
    if (!t) return;
    if (act === "toggle") return update({ ...t, done: el.checked });
    if (act === "del") return invoke("delete_task", { id });
    if (act === "snooze-h") return update({ ...t, due: new Date(Date.now() + 36e5).toISOString(), has_time: true, notified: false });
    if (act === "snooze-d") {
      const d = new Date();
      d.setDate(d.getDate() + 1);
      d.setHours(9, 0, 0, 0);
      return update({ ...t, due: d.toISOString(), has_time: true, notified: false });
    }
    if (act === "edit-due") {
      editing = id;
      renderTasks();
      $(".t-due-edit")?.focus();
      return;
    }
    if (act === "clear-due") {
      editing = null;
      return update({ ...t, due: null, has_time: false, notified: false });
    }
  });
  $("#task-list").addEventListener("change", (e) => {
    if (!e.target.classList.contains("t-due-edit")) return;
    const id = e.target.closest("[data-id]").dataset.id;
    const t = tasks.find((x) => x.id === id);
    editing = null;
    if (t && e.target.value) update({ ...t, due: new Date(e.target.value).toISOString(), has_time: true, notified: false });
    else renderTasks();
  });
  $("#task-list").addEventListener("focusout", (e) => {
    if (e.target.classList.contains("t-due-edit")) {
      setTimeout(() => {
        if (editing && !document.activeElement?.closest?.(".t-due-box")) {
          editing = null;
          renderTasks();
        }
      }, 150);
    }
  });
  $("#task-list").addEventListener("dblclick", (e) => {
    const el = e.target.closest(".t-text");
    if (!el) return;
    el.contentEditable = "true";
    el.focus();
    document.getSelection().selectAllChildren(el);
    const done = async (save) => {
      el.contentEditable = "false";
      const id = el.closest("[data-id]").dataset.id;
      const t = tasks.find((x) => x.id === id);
      const text = el.textContent.trim();
      if (save && t && text && text !== t.text) await update({ ...t, text });
      else renderTasks();
    };
    el.onkeydown = (k) => {
      if (k.key === "Enter") (k.preventDefault(), done(true));
      if (k.key === "Escape") done(false);
    };
    el.onblur = () => done(true);
  });

  async function addFromInput() {
    const v = $("#task-input").value.trim();
    if (!v) return;
    $("#task-input").value = "";
    try {
      await invoke("add_task", { text: v, smart: true, source: "manual", sourceId: null });
    } catch (e) {
      toast(String(e));
    }
  }
  $("#task-add").addEventListener("click", addFromInput);
  $("#task-input").addEventListener("keydown", (e) => e.key === "Enter" && addFromInput());

  $("#briefing-refresh").addEventListener("click", async () => {
    try {
      await invoke("refresh_briefing");
    } catch (e) {
      toast(String(e));
    }
  });
  $("#briefing-body").addEventListener("click", (e) => {
    const a = e.target.closest("a[data-url]");
    if (a) {
      e.preventDefault();
      invoke("plugin:opener|open_url", { url: a.dataset.url }).catch(() => {});
    }
  });

  async function brief() {
    const q = $("#brief-input").value.trim();
    if (!q) return $("#brief-input").focus();
    $("#brief-input").value = "";
    await invoke("ask_open_with", { text: `/brief ${q}`, send: true }).catch((e) => toast(String(e)));
  }
  $("#brief-go").addEventListener("click", brief);
  $("#brief-input").addEventListener("keydown", (e) => e.key === "Enter" && brief());

  listen("whiskr:tasks", (ev) => {
    tasks = ev.payload || [];
    renderTasks();
  });
  listen("whiskr:briefing", (ev) => {
    briefing = ev.payload;
    renderBriefing();
  });
  listen("whiskr:briefing-busy", (ev) => {
    busy = !!ev.payload;
    renderBriefing();
  });
  // keep "today / overdue" fresh
  setInterval(() => {
    if (!$("#task-list").contains(document.activeElement)) renderTasks();
  }, 60e3);

  return {
    async init() {
      try {
        [tasks, briefing] = await Promise.all([invoke("list_tasks"), invoke("get_briefing")]);
      } catch {}
      render();
    },
    render,
  };
}
