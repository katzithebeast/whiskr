// Settings → 🧠 Asistent: who thinks (Claude / Groq), which account, the
// morning briefing (time, connectors, own instructions), call detection.

export function initAssistantSettings(ctx) {
  const { invoke, toast, esc, $, $$, T, settings, save } = ctx;
  let servers = null; // null = not loaded yet
  let loading = false;

  const fill = (s, o) => s.replace(/\{(\w+)\}/g, (_, k) => o[k] ?? "");

  async function renderStatus() {
    const t = T();
    try {
      const st = await invoke("brain_status");
      $("#as-brain-status").textContent = st.engine
        ? fill(t.as_brain_now, { e: st.engine === "Claude" && st.profile ? `Claude · ${st.profile}` : st.engine }) + (st.engine === "Claude" && st.fallback ? t.as_brain_fb : "")
        : t.as_brain_none;
    } catch {}
  }

  function renderTools() {
    const t = T();
    const s = settings();
    const all = s.briefing_all_tools !== false;
    $$('input[name="as-tools"]').forEach((r) => (r.checked = r.value === (all ? "all" : "some")));
    const box = $("#as-tools-list");
    const known = new Set([...(servers || []), ...(s.briefing_tools || [])]);
    if (loading) return (box.innerHTML = `<span class="muted small">${esc(t.as_tools_loading)}</span>`);
    if (servers && !known.size) return (box.innerHTML = `<span class="muted small">${esc(t.as_tools_none)}</span>`);
    box.innerHTML = [...known]
      .map((n) => {
        const on = all || (s.briefing_tools || []).includes(n);
        return `<label class="pick${all ? " dim" : ""}"><input type="checkbox" value="${esc(n)}"${on ? " checked" : ""}${all ? " disabled" : ""}/> ${esc(n)}</label>`;
      })
      .join("");
  }

  function render() {
    const t = T();
    const s = settings();
    $("#as-brain").innerHTML = ["auto", "claude", "groq"].map((k) => `<option value="${k}">${esc(t["as_brain_" + k])}</option>`).join("");
    $("#as-brain").value = s.brain || "auto";
    const profs = s.profiles || [];
    $("#as-profile").innerHTML = profs.map((p) => `<option value="${esc(p.id)}">${esc(p.name)}</option>`).join("");
    $("#as-profile").value = s.brain_profile && profs.some((p) => p.id === s.brain_profile) ? s.brain_profile : s.active_profile || profs[0]?.id || "";
    const models = [["", t.as_model_default], ["sonnet", "Sonnet"], ["opus", "Opus"], ["haiku", "Haiku"]];
    $("#as-model").innerHTML = models.map(([v, n]) => `<option value="${v}">${esc(n)}</option>`).join("");
    $("#as-model").value = s.brain_model || "";
    const claudeOff = (s.brain || "auto") === "groq";
    $("#as-profile").disabled = $("#as-model").disabled = claudeOff;
    $("#as-briefing").checked = s.briefing !== false;
    $("#as-btime").value = s.briefing_time || "08:00";
    $("#as-bopen").checked = s.briefing_open !== false;
    if (document.activeElement !== $("#as-extra")) $("#as-extra").value = s.briefing_extra || "";
    $("#as-detect").checked = s.meeting_detect !== false;
    $("#as-autostop").checked = s.meeting_autostop !== false;
    $("#as-autostop").disabled = !$("#as-detect").checked;
    renderTools();
    renderStatus();
  }

  async function loadServers() {
    loading = true;
    renderTools();
    try {
      servers = await invoke("brain_connectors");
    } catch (e) {
      servers = [];
      toast(String(e));
    }
    loading = false;
    renderTools();
  }

  $("#as-brain").addEventListener("change", async (e) => {
    await save({ brain: e.target.value });
    render();
  });
  $("#as-profile").addEventListener("change", async (e) => {
    await save({ brain_profile: e.target.value });
    servers = null;
    render();
  });
  $("#as-model").addEventListener("change", (e) => save({ brain_model: e.target.value }));
  $("#as-briefing").addEventListener("change", (e) => save({ briefing: e.target.checked }));
  $("#as-btime").addEventListener("change", (e) => /^\d\d:\d\d$/.test(e.target.value) && save({ briefing_time: e.target.value }));
  $("#as-bopen").addEventListener("change", (e) => save({ briefing_open: e.target.checked }));
  $("#as-extra").addEventListener("change", (e) => save({ briefing_extra: e.target.value.trim() }, true));
  $("#as-detect").addEventListener("change", async (e) => {
    await save({ meeting_detect: e.target.checked });
    render();
  });
  $("#as-autostop").addEventListener("change", (e) => save({ meeting_autostop: e.target.checked }));
  $("#as-tools-load").addEventListener("click", loadServers);
  $$('input[name="as-tools"]').forEach((r) =>
    r.addEventListener("change", async () => {
      const all = r.value === "all";
      const patch = { briefing_all_tools: all };
      if (!all && !(settings().briefing_tools || []).length && servers) patch.briefing_tools = [...servers];
      await save(patch, true);
      if (!all && servers === null) loadServers();
      renderTools();
    })
  );
  $("#as-tools-list").addEventListener("change", async () => {
    const picked = $$("#as-tools-list input:checked").map((i) => i.value);
    await save({ briefing_tools: picked }, true);
  });

  return {
    init: render,
    render,
  };
}
