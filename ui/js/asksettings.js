// Settings → Whiskr Ask: detected CLIs + logins, profiles (personal / work),
// projects for delegating, default model, where to run, agent access.

const TOOLS = [
  { engine: "claude", name: "Claude Code", sub: "Claude Pro / Max", install: "npm i -g @anthropic-ai/claude-code" },
  { engine: "codex", name: "Codex", sub: "ChatGPT Plus / Pro", install: "npm i -g @openai/codex" },
  { engine: "cursor", name: "Cursor Agent", sub: "Cursor", install: "curl https://cursor.com/install -fsS | bash" },
];

export function initAskSettings(ctx) {
  const { invoke, listen, toast, esc, $, $$, T, settings, save } = ctx;
  let info = { tools: [], groq: false };
  let checking = false;

  function profiles() {
    return settings().profiles || [];
  }

  function renderTools() {
    if (!$("#ask-tools")) return; // moved to Settings → Connections
    const t = T();
    const s = settings();
    const active = profiles().find((p) => p.id === s.active_profile) || profiles()[0];
    $("#ask-tools").innerHTML = TOOLS.map((tool) => {
      const found = info.tools.filter((x) => x.engine === tool.engine);
      const chips = found.length
        ? found
            .map((f) => {
              const rt = f.runtime === "wsl" ? "WSL" : "Windows";
              const st = f.logged_in === true ? `<span class="ok">✓ ${esc(t.tool_logged)}</span>` : f.logged_in === false ? `<span class="no">${esc(t.tool_not_logged)}</span>` : "";
              const login = `<button class="btn btn-ghost btn-sm" data-login="${tool.engine}" data-rt="${f.runtime}">${esc(t.ask_login)}${profiles().length > 1 && active ? ` · ${esc(active.name)}` : ""}</button>`;
              return `<span class="tool-rt" title="${esc(f.path)}"><b>${rt}</b>${st}${login}</span>`;
            })
            .join("")
        : `<span class="muted small">${esc(t.tool_missing)} · ${esc(t.tool_install)} <code class="copy" data-copy="${esc(tool.install)}">${esc(tool.install)}</code></span>`;
      return `<div class="tool-row${found.length ? "" : " missing"}"><div><b>${tool.name}</b><div class="muted small">${tool.sub}</div></div><div class="tool-chips">${chips}</div></div>`;
    }).join("") + `<div class="tool-row${info.groq ? "" : " missing"}"><div><b>Groq</b><div class="muted small">zdarma · ${info.groq ? "✓" : "—"}</div></div><div class="tool-chips"><span class="muted small">${info.groq ? "API klíč z nastavení výše" : ""}</span></div></div>`;
  }

  function renderSelects() {
    const t = T();
    const s = settings();
    const names = { claude: "Claude (Claude Code)", codex: "GPT (Codex)", cursor: "Cursor Agent", groq: "Groq — rychlý, zdarma" };
    $("#ask-engine").innerHTML = Object.entries(names).map(([k, n]) => `<option value="${k}">${esc(n)}</option>`).join("");
    $("#ask-engine").value = s.ask_engine || "claude";
    $("#ask-runtime").innerHTML = ["auto", "windows", "wsl"].map((k) => `<option value="${k}">${esc(t["rt_" + k])}</option>`).join("");
    $("#ask-runtime").value = s.ask_runtime || "auto";
    $("#ask-sel").checked = s.ask_selection !== false;
    if ($("#ask-tw")) $("#ask-tw").checked = !!s.ask_typewriter;
    if ($("#ask-fast")) $("#ask-fast").checked = s.ask_fast !== false;
    if ($("#ask-swap")) $("#ask-swap").checked = !!s.ask_space_new;
    if ($("#ask-vsend")) $("#ask-vsend").checked = s.ask_voice_send !== false;
    if ($("#ask-tw-speed")) {
      const cps = s.ask_type_cps || 90, ms = s.ask_anim_ms || 620;
      $("#ask-tw-speed").value = cps;
      $("#ask-tw-speed-v").textContent = `${cps} ${T().u_cps}`;
      $("#ask-tw-speed").disabled = !s.ask_typewriter;
      $("#ask-anim").value = ms;
      $("#ask-anim-v").textContent = `${(ms / 1000).toFixed(2).replace(/0$/, "")} s`;
    }
    $$("#ask-access .seg").forEach((b) => b.classList.toggle("on", b.dataset.v === (s.ask_access || "safe")));
  }

  function renderProfiles() {
    return; // Settings → Connections owns profiles now
    const t = T();
    const s = settings();
    $("#ask-profiles").innerHTML = profiles()
      .map(
        (p, i) => `<div class="prof-item" data-i="${i}">
          <label class="radio"><input type="radio" name="ask-active" value="${esc(p.id)}"${p.id === s.active_profile ? " checked" : ""}/> <span>${esc(t.ask_active)}</span></label>
          <input class="input input-sm" data-k="name" value="${esc(p.name)}" />
          <input class="input input-sm mono" data-k="key" value="${esc(p.key)}" placeholder="${esc(t.ask_profile_default)}" title="~/.claude-<key>, ~/.codex-<key>" />
          <button class="btn btn-ghost btn-sm" data-del="${i}"${profiles().length < 2 ? " disabled" : ""}>${esc(t.tpl_delete)}</button>
        </div>`
      )
      .join("");
  }

  function renderProjects() {
    const t = T();
    const ps = settings().projects || [];
    const opts = (sel) =>
      `<option value="">${esc(t.ask_profile_any)}</option>` + profiles().map((p) => `<option value="${esc(p.id)}"${p.id === sel ? " selected" : ""}>${esc(p.name)}</option>`).join("");
    $("#ask-projects").innerHTML = ps
      .map(
        (p, i) => `<div class="proj-item" data-i="${i}">
          <input class="input input-sm" data-k="name" value="${esc(p.name)}" />
          <span class="muted small path" title="${esc(p.path)}">${esc(p.path)}</span>
          <select class="input input-sm" data-k="profile">${opts(p.profile)}</select>
          <button class="btn btn-ghost btn-sm" data-del="${i}">${esc(t.tpl_delete)}</button>
        </div>`
      )
      .join("");
  }

  function render() {
    renderTools();
    renderSelects();
    renderProfiles();
    renderProjects();
  }

  async function load(refresh, checkLogin) {
    try {
      info = await invoke("ask_tools", { refresh, checkLogin });
    } catch (e) {
      toast(String(e));
    }
    renderTools();
  }

  $("#ask-refresh")?.addEventListener("click", async (e) => {
    e.target.disabled = true;
    await load(true, false);
    e.target.disabled = false;
  });
  $("#ask-check")?.addEventListener("click", async (e) => {
    if (checking) return;
    checking = true;
    e.target.disabled = true;
    await load(false, true);
    e.target.disabled = false;
    checking = false;
  });
  $("#ask-tools")?.addEventListener("click", async (e) => {
    const c = e.target.closest("[data-copy]");
    if (c) {
      await invoke("copy_text", { text: c.dataset.copy });
      return toast("Zkopírováno ✓");
    }
    const b = e.target.closest("[data-login]");
    if (!b) return;
    try {
      await invoke("ask_login", { engine: b.dataset.login, runtime: b.dataset.rt, profile: settings().active_profile || "personal" });
    } catch (err) {
      toast(String(err));
    }
  });
  $("#ask-engine").addEventListener("change", (e) => save({ ask_engine: e.target.value }));
  $("#ask-runtime").addEventListener("change", (e) => save({ ask_runtime: e.target.value }));
  $("#ask-sel").addEventListener("change", (e) => save({ ask_selection: e.target.checked }));
  $("#ask-tw")?.addEventListener("change", async (e) => {
    await save({ ask_typewriter: e.target.checked });
    renderSelects();
  });
  $("#ask-fast")?.addEventListener("change", (e) => save({ ask_fast: e.target.checked }));
  $("#ask-swap")?.addEventListener("change", (e) => save({ ask_space_new: e.target.checked }));
  $("#ask-vsend")?.addEventListener("change", (e) => save({ ask_voice_send: e.target.checked }));
  $("#ask-tw-speed")?.addEventListener("input", (e) => ($("#ask-tw-speed-v").textContent = `${e.target.value} ${T().u_cps}`));
  $("#ask-tw-speed")?.addEventListener("change", (e) => save({ ask_type_cps: Number(e.target.value) }, true));
  $("#ask-anim")?.addEventListener("input", (e) => ($("#ask-anim-v").textContent = `${(e.target.value / 1000).toFixed(2).replace(/0$/, "")} s`));
  $("#ask-anim")?.addEventListener("change", (e) => save({ ask_anim_ms: Number(e.target.value) }, true));
  $$("#ask-access .seg").forEach((b) =>
    b.addEventListener("click", async () => {
      await save({ ask_access: b.dataset.v });
      renderSelects();
    })
  );

  // profiles
  $("#ask-profiles-old")?.addEventListener("change", async (e) => {
    if (e.target.name === "ask-active") {
      await save({ active_profile: e.target.value }, true);
      return renderTools();
    }
    const f = e.target.closest("[data-k]");
    if (!f) return;
    const i = Number(f.closest(".prof-item").dataset.i);
    const list = profiles().map((p) => ({ ...p }));
    let v = f.value.trim();
    if (f.dataset.k === "key") v = v.toLowerCase().replace(/[^a-z0-9_-]/g, "");
    list[i][f.dataset.k] = v;
    if (await save({ profiles: list }, true)) render();
  });
  $("#ask-profiles-old")?.addEventListener("click", async (e) => {
    const b = e.target.closest("[data-del]");
    if (!b) return;
    const list = profiles().filter((_, i) => i !== Number(b.dataset.del));
    const s = settings();
    const patch = { profiles: list };
    if (!list.some((p) => p.id === s.active_profile)) patch.active_profile = list[0]?.id || "personal";
    if (await save(patch, true)) render();
  });
  $("#ask-profile-add-old")?.addEventListener("click", async () => {
    const id = "p" + Date.now().toString(36);
    const list = [...profiles(), { id, name: "Klient", key: "client" + (profiles().length + 1) }];
    if (await save({ profiles: list }, true)) render();
  });

  // projects
  $("#ask-project-add").addEventListener("click", async () => {
    const path = await invoke("pick_folder").catch(() => null);
    if (!path) return;
    const name = path.split(/[\\/]/).filter(Boolean).pop() || path;
    const list = [...(settings().projects || []).filter((p) => p.path !== path), { name, path, profile: "" }];
    if (await save({ projects: list }, true)) renderProjects();
  });
  $("#ask-projects").addEventListener("change", async (e) => {
    const f = e.target.closest("[data-k]");
    if (!f) return;
    const i = Number(f.closest(".proj-item").dataset.i);
    const list = (settings().projects || []).map((p) => ({ ...p }));
    list[i][f.dataset.k] = f.value;
    await save({ projects: list }, true);
  });
  $("#ask-projects").addEventListener("click", async (e) => {
    const b = e.target.closest("[data-del]");
    if (!b) return;
    const list = (settings().projects || []).filter((_, i) => i !== Number(b.dataset.del));
    if (await save({ projects: list }, true)) renderProjects();
  });

  listen("whiskr:tools", (ev) => {
    info.tools = ev.payload || [];
    renderTools();
  });

  return {
    async init() {
      render();
      await load(false, false);
    },
    render,
  };
}
