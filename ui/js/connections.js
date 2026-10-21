// Settings → Připojení: logins for Claude Code / Codex / Cursor Agent (per
// profile) and one MCP library you can hand to any of them with a click.

const ENGINES = {
  claude: { name: "Claude Code", sub: "Claude Pro / Max", color: "#E08A5C", install: "npm i -g @anthropic-ai/claude-code" },
  codex: { name: "Codex", sub: "ChatGPT Plus / Pro", color: "#3FBF8F", install: "npm i -g @openai/codex" },
  cursor: { name: "Cursor Agent", sub: "Cursor", color: "#B8BCC6", install: "curl https://cursor.com/install -fsS | bash" },
};
const TARGETS = [
  ["claude", "Claude Code"],
  ["codex", "Codex"],
  ["cursor", "Cursor"],
  ["desktop", "Claude Desktop"],
];

// "npx -y @x/mcp --flag \"a b\"" → ["npx", "-y", "@x/mcp", "--flag", "a b"]
function splitCmd(s) {
  const out = [];
  let cur = "", q = null, any = false;
  for (const c of s.trim()) {
    if (q) {
      if (c === q) q = null;
      else cur += c;
    } else if (c === '"' || c === "'") {
      q = c;
      any = true;
    } else if (/\s/.test(c)) {
      if (cur || any) out.push(cur);
      cur = "";
      any = false;
    } else cur += c;
  }
  if (cur || any) out.push(cur);
  return out;
}

export function initConnections(ctx) {
  const { invoke, toast, esc, $, T, settings, save } = ctx;
  let acc = null;
  let accBusy = false;
  let mcp = null;
  let mcpBusy = false;
  let pending = new Set(); // "name:target" toggles in flight
  let profile = null;

  const profiles = () => settings().profiles || [];
  const lib = () => settings().mcp_library || [];

  // ───────── accounts ─────────
  function renderAccounts() {
    const t = T();
    const box = $("#cn-accounts");
    if (!acc) {
      box.innerHTML = `<div class="wait"><span class="spin"></span>${esc(t.cn_checking)}</div>`;
      return;
    }
    box.innerHTML = acc.accounts
      .map((a) => {
        const e = ENGINES[a.engine];
        const where = a.tools.map((x) => (x.runtime === "wsl" ? "WSL" : "Windows")).join(" + ");
        let rows;
        if (!a.tools.length) {
          rows = `<div class="acc-row"><span class="muted small">${esc(t.tool_missing)} · ${esc(t.tool_install)}</span> <code class="copy" data-copy="${esc(e.install)}">${esc(e.install)}</code></div>`;
        } else {
          const list = a.engine === "cursor" ? [{ id: profiles()[0]?.id, name: "" }] : profiles();
          rows = list
            .map((p) => {
              const st = a.logins[p.id];
              const badge = st === true ? `<span class="ok">✓ ${esc(t.tool_logged)}</span>` : st === false ? `<span class="no">${esc(t.tool_not_logged)}</span>` : `<span class="muted small">?</span>`;
              const btn =
                st === true
                  ? `<button class="btn btn-ghost btn-sm" data-logout="${a.engine}" data-p="${esc(p.id)}">${esc(t.cn_logout)}</button>`
                  : `<button class="btn btn-dark btn-sm" data-login="${a.engine}" data-p="${esc(p.id)}" data-rt="${esc(a.tools[0].runtime)}">${esc(t.ask_login)}</button>`;
              return `<div class="acc-row">${p.name ? `<b class="acc-prof">${esc(p.name)}</b>` : ""}${badge}<span class="grow"></span>${btn}</div>`;
            })
            .join("");
        }
        return `<div class="acc">
          <div class="acc-head"><i style="background:${e.color}"></i><div><b>${esc(e.name)}</b><div class="muted small">${esc(e.sub)}${where ? " · " + esc(where) : ""}</div></div></div>
          <div class="acc-rows">${rows}</div>
        </div>`;
      })
      .join("");
  }

  async function loadAccounts(refresh) {
    if (accBusy) return;
    accBusy = true;
    if (refresh) acc = null;
    renderAccounts();
    try {
      acc = await invoke("connections_status", { refresh: !!refresh });
    } catch (e) {
      toast(String(e));
    }
    accBusy = false;
    renderAccounts();
  }

  $("#cn-accounts").addEventListener("click", async (e) => {
    const c = e.target.closest("[data-copy]");
    if (c) {
      await invoke("copy_text", { text: c.dataset.copy });
      return toast(T().copied || "✓");
    }
    const li = e.target.closest("[data-login]");
    if (li) {
      try {
        await invoke("ask_login", { engine: li.dataset.login, runtime: li.dataset.rt, profile: li.dataset.p });
        toast(T().cn_login_hint);
        // check again after the user had time to finish in the browser
        setTimeout(() => loadAccounts(false), 25000);
      } catch (err) {
        toast(String(err));
      }
      return;
    }
    const lo = e.target.closest("[data-logout]");
    if (lo) {
      lo.disabled = true;
      try {
        await invoke("account_logout", { engine: lo.dataset.logout, profile: lo.dataset.p });
      } catch (err) {
        toast(String(err));
      }
      loadAccounts(false);
    }
  });
  $("#cn-refresh").addEventListener("click", () => loadAccounts(true));

  // ───────── profiles (personal / work) ─────────
  function renderProfiles() {
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
    const sel = $("#cn-profile");
    sel.innerHTML = profiles().map((p) => `<option value="${esc(p.id)}">${esc(p.name)}</option>`).join("");
    if (!profile || !profiles().some((p) => p.id === profile)) profile = s.active_profile || profiles()[0]?.id;
    sel.value = profile;
  }
  $("#ask-profiles").addEventListener("change", async (e) => {
    if (e.target.name === "ask-active") return save({ active_profile: e.target.value }, true);
    const f = e.target.closest("[data-k]");
    if (!f) return;
    const i = Number(f.closest(".prof-item").dataset.i);
    const list = profiles().map((p) => ({ ...p }));
    let v = f.value.trim();
    if (f.dataset.k === "key") v = v.toLowerCase().replace(/[^a-z0-9_-]/g, "");
    list[i][f.dataset.k] = v;
    if (await save({ profiles: list }, true)) {
      renderProfiles();
      loadAccounts(false);
    }
  });
  $("#ask-profiles").addEventListener("click", async (e) => {
    const b = e.target.closest("[data-del]");
    if (!b) return;
    const list = profiles().filter((_, i) => i !== Number(b.dataset.del));
    const patch = { profiles: list };
    if (!list.some((p) => p.id === settings().active_profile)) patch.active_profile = list[0]?.id || "personal";
    if (await save(patch, true)) renderProfiles();
  });
  $("#ask-profile-add").addEventListener("click", async () => {
    const id = "p" + Date.now().toString(36);
    const list = [...profiles(), { id, name: "Klient", key: "client" + (profiles().length + 1) }];
    if (await save({ profiles: list }, true)) {
      renderProfiles();
      loadAccounts(false);
    }
  });

  // ───────── MCP library ─────────
  const detail = (d) => (d.kind === "http" ? d.url : [d.command, ...(d.args || [])].join(" "));

  function renderMcp() {
    const t = T();
    const box = $("#cn-mcp");
    if (!mcp) {
      box.innerHTML = `<div class="wait"><span class="spin"></span>${esc(t.cn_checking)}</div>`;
      $("#cn-found").innerHTML = "";
      $("#cn-claudeai").innerHTML = "";
      return;
    }
    const rows = [{ ...mcp.whiskr, builtin: true }, ...lib()];
    box.innerHTML = rows
      .map((d, i) => {
        const chips = TARGETS.map(([k, label]) => {
          const tg = mcp.targets[k] || {};
          const on = (tg.installed || []).includes(d.name);
          const busy = pending.has(d.name + ":" + k);
          const dis = !tg.available;
          return `<button class="tgt${on ? " on" : ""}${busy ? " busy" : ""}" data-tgt="${k}" data-i="${d.builtin ? -1 : i - 1}"${dis ? ` disabled title="${esc(tg.note || "")}"` : ""}>${busy ? '<span class="spin"></span>' : on ? "✓" : "+"} ${esc(label)}</button>`;
        }).join("");
        return `<div class="mcp-row">
          <div class="mcp-info"><b>${esc(d.name)}</b>${d.builtin ? `<span class="tag">Whiskr</span>` : `<span class="tag">${d.kind === "http" ? "URL" : "stdio"}</span>`}
            <div class="muted small mono ell" title="${esc(detail(d))}">${esc(d.builtin ? t.cn_whiskr_sub : detail(d))}</div></div>
          <div class="tgts">${chips}</div>
          ${d.builtin ? "" : `<button class="icon-btn" data-rm="${i - 1}" title="${esc(t.t_del)}">✕</button>`}
        </div>`;
      })
      .join("");
    // servers set up elsewhere but not in the library
    const known = new Set(["whiskr", ...lib().map((d) => d.name)]);
    const found = (mcp.found || []).filter((d) => !known.has(d.name));
    const nameOnly = [...new Set([...(mcp.targets.claude?.installed || []), ...(mcp.targets.codex?.installed || [])])].filter((n) => !known.has(n) && !found.some((f) => f.name === n));
    $("#cn-found").innerHTML =
      found.length || nameOnly.length
        ? `<div class="sub-h">${esc(t.cn_found)}</div>` +
          found.map((d, i) => `<div class="mcp-row slim"><div class="mcp-info"><b>${esc(d.name)}</b><div class="muted small mono ell">${esc(detail(d))}</div></div><button class="btn btn-light btn-sm" data-import="${i}">${esc(t.cn_import)}</button></div>`).join("") +
          nameOnly.map((n) => `<div class="mcp-row slim"><div class="mcp-info"><b>${esc(n)}</b><div class="muted small">${esc(t.cn_only_cli)}</div></div></div>`).join("")
        : "";
    $("#cn-found").dataset.list = JSON.stringify(found);
    $("#cn-claudeai").innerHTML = (mcp.claude_ai || []).length
      ? `<div class="sub-h">${esc(t.cn_claudeai)}</div><div class="chips" style="margin-top:6px">${mcp.claude_ai.map((n) => `<span class="pick dim">${esc(n.replace(/^claude\.ai /, ""))}</span>`).join("")}</div><div class="muted small" style="margin-top:6px">${esc(t.cn_claudeai_sub)}</div>`
      : "";
  }

  async function loadMcp() {
    if (mcpBusy) return;
    mcpBusy = true;
    mcp = null;
    renderMcp();
    try {
      mcp = await invoke("mcp_status", { profile });
    } catch (e) {
      toast(String(e));
      mcp = { targets: {}, found: [], claude_ai: [], whiskr: { name: "whiskr", kind: "stdio", command: "whiskr-mcp.exe", args: [] } };
    }
    mcpBusy = false;
    renderMcp();
  }

  $("#cn-mcp").addEventListener("click", async (e) => {
    const rm = e.target.closest("[data-rm]");
    if (rm) {
      const list = lib().filter((_, i) => i !== Number(rm.dataset.rm));
      if (await save({ mcp_library: list }, true)) renderMcp();
      return;
    }
    const b = e.target.closest("[data-tgt]");
    if (!b || b.disabled) return;
    const i = Number(b.dataset.i);
    const def = i < 0 ? mcp.whiskr : lib()[i];
    const target = b.dataset.tgt;
    const on = !b.classList.contains("on");
    const key = def.name + ":" + target;
    if (pending.has(key)) return;
    pending.add(key);
    renderMcp();
    try {
      await invoke("mcp_install", { def, target, profile, on });
      const tg = mcp.targets[target];
      tg.installed = on ? [...new Set([...(tg.installed || []), def.name])] : (tg.installed || []).filter((n) => n !== def.name);
      toast(on ? T().cn_added.replace("{t}", b.textContent.replace(/^[✓+]\s*/, "")) : T().cn_removed);
    } catch (err) {
      toast(String(err));
    }
    pending.delete(key);
    renderMcp();
  });
  $("#cn-found").addEventListener("click", async (e) => {
    const b = e.target.closest("[data-import]");
    if (!b) return;
    const list = JSON.parse($("#cn-found").dataset.list || "[]");
    const d = list[Number(b.dataset.import)];
    if (d && (await save({ mcp_library: [...lib(), d] }, true))) renderMcp();
  });
  $("#cn-profile").addEventListener("change", (e) => {
    profile = e.target.value;
    loadMcp();
  });
  $("#cn-mcp-refresh").addEventListener("click", loadMcp);

  // add form
  $("#cn-mcp-add").addEventListener("click", () => {
    $("#cn-mcp-form").hidden = false;
    $("#cn-mcp-add").hidden = true;
    $("#mf-name").focus();
  });
  const closeForm = () => {
    $("#cn-mcp-form").hidden = true;
    $("#cn-mcp-add").hidden = false;
    for (const id of ["#mf-name", "#mf-cmd", "#mf-env"]) $(id).value = "";
  };
  $("#mf-cancel").addEventListener("click", closeForm);
  $("#mf-kind").addEventListener("change", () => {
    $("#mf-cmd").placeholder = $("#mf-kind").value === "http" ? "https://…/mcp" : T().cn_f_cmd;
    $("#mf-env").hidden = $("#mf-kind").value === "http";
  });
  $("#mf-save").addEventListener("click", async () => {
    const t = T();
    const name = $("#mf-name").value.trim().replace(/\s+/g, "-");
    const kind = $("#mf-kind").value;
    const raw = $("#mf-cmd").value.trim();
    if (!name || !raw) return toast(t.cn_f_missing);
    if (name === "whiskr" || lib().some((d) => d.name === name)) return toast(t.cn_f_dupe);
    let def;
    if (kind === "http") def = { name, kind, url: raw, command: "", args: [], env: [] };
    else {
      const parts = splitCmd(raw);
      def = { name, kind, command: parts[0], args: parts.slice(1), url: "", env: $("#mf-env").value.split("\n").map((l) => l.trim()).filter((l) => l.includes("=")) };
    }
    if (await save({ mcp_library: [...lib(), def] }, true)) {
      closeForm();
      renderMcp();
    }
  });

  let loaded = false;
  return {
    render() {
      renderProfiles();
      renderAccounts();
      renderMcp();
    },
    // load lazily the first time the tab is opened (CLI checks take a few seconds)
    open() {
      renderProfiles();
      if (loaded) return;
      loaded = true;
      loadAccounts(false);
      loadMcp();
    },
    init() {
      renderProfiles();
    },
  };
}
