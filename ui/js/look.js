// Background of the app and the chat: colour, own image, blur, dim, opacity,
// and saved presets ("Výchozí" = how Whiskr looks out of the box).

export const DEFAULT_LOOK = { color: "", image: "", blur: 0, dim: 35, opacity: 100 };

function hexA(hex, a) {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex || "");
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${a})`;
}

const same = (a, b) => ["color", "image", "blur", "dim", "opacity"].every((k) => (a?.[k] ?? DEFAULT_LOOK[k]) === (b?.[k] ?? DEFAULT_LOOK[k]));

export function initLook(ctx) {
  const { invoke, toast, esc, $, $$, T, settings, save } = ctx;
  const TAURI = window.__TAURI__;
  let draft = null; // live values while dragging a slider
  let glassOn = null;

  const look = () => ({ ...DEFAULT_LOOK, ...(draft || settings().look || {}) });

  // ───────── apply to the main window ─────────
  function apply() {
    const L = look();
    const op = Math.min(100, Math.max(30, L.opacity)) / 100;
    const dark = document.documentElement.dataset.theme === "dark";
    const base = L.color || (dark ? "#0E0E0F" : "#F8F6EF");
    const src = L.image && TAURI?.core?.convertFileSrc ? TAURI.core.convertFileSrc(L.image) : "";
    $("#look-bg").style.background = hexA(base, op);
    const img = $("#look-bg-img");
    img.style.backgroundImage = src ? `url("${src}")` : "none";
    img.style.filter = L.blur ? `blur(${L.blur}px)` : "none";
    img.style.transform = L.blur ? "scale(1.06)" : "none";
    img.style.opacity = src ? op : 0;
    $("#look-bg-dim").style.background = src ? hexA(base, (L.dim / 100) * op) : "transparent";
    document.body.classList.toggle("looked", !!(src || L.color || op < 1));
    // a plain colour + blur = frosted glass: Windows blurs the desktop behind
    const glass = !src && L.blur > 0 && op < 1;
    document.body.classList.toggle("glass", glass);
    if (glass !== glassOn) {
      glassOn = glass;
      invoke("set_glass", { window: "main", on: glass }).catch(() => {});
    }
  }

  // ───────── settings card ─────────
  function render() {
    const t = T();
    const L = look();
    const presets = settings().look_presets || [];
    const chips = [{ name: t.lk_default, look: DEFAULT_LOOK, builtin: true }, ...presets]
      .map(
        (p, i) =>
          `<span class="lk-chip${same(p.look, settings().look) ? " on" : ""}" data-p="${p.builtin ? -1 : i - 1}">${esc(p.name)}${
            p.builtin ? "" : `<button data-del="${i - 1}" title="${esc(t.t_del)}">✕</button>`
          }</span>`
      )
      .join("");
    $("#lk-presets").innerHTML = chips + `<button class="btn btn-light btn-sm" id="lk-save">${esc(t.lk_save)}</button>`;
    $("#lk-color").value = L.color || (document.documentElement.dataset.theme === "dark" ? "#0e0e0f" : "#f8f6ef");
    $("#lk-color-reset").hidden = !L.color;
    const src = L.image && TAURI?.core?.convertFileSrc ? TAURI.core.convertFileSrc(L.image) : "";
    $("#lk-thumb").style.backgroundImage = src ? `url("${src}")` : "none";
    $("#lk-thumb").classList.toggle("empty", !src);
    $("#lk-img-del").hidden = !src;
    for (const [id, k, unit] of [["lk-blur", "blur", " px"], ["lk-dim", "dim", " %"], ["lk-op", "opacity", " %"]]) {
      $("#" + id).value = L[k];
      $("#" + id + "-v").textContent = L[k] + unit;
    }
    $("#lk-dim").disabled = !src;
    $("#lk-blur-d").textContent = src ? t.lk_blur_img : t.lk_blur_glass;
  }

  async function commit(patch) {
    draft = null;
    const next = { ...look(), ...patch };
    if (await save({ look: next }, true)) {
      apply();
      render();
    }
  }

  for (const [id, k, unit] of [["lk-blur", "blur", " px"], ["lk-dim", "dim", " %"], ["lk-op", "opacity", " %"]]) {
    $("#" + id).addEventListener("input", (e) => {
      draft = { ...look(), [k]: Number(e.target.value) };
      $("#" + id + "-v").textContent = e.target.value + unit;
      apply();
    });
    $("#" + id).addEventListener("change", (e) => {
      const patch = { [k]: Number(e.target.value) };
      // blur on a plain colour only shows through some transparency
      const L = look();
      if (k === "blur" && patch.blur > 0 && !L.image && L.opacity >= 100) {
        patch.opacity = 75;
        toast(T().lk_auto_op);
      }
      commit(patch);
    });
  }
  $("#lk-color").addEventListener("input", (e) => {
    draft = { ...look(), color: e.target.value };
    apply();
  });
  $("#lk-color").addEventListener("change", (e) => commit({ color: e.target.value }));
  $("#lk-color-reset").addEventListener("click", () => commit({ color: "" }));
  $("#lk-img").addEventListener("click", async () => {
    try {
      const path = await invoke("pick_background");
      if (path) commit({ image: path });
    } catch (e) {
      toast(String(e));
    }
  });
  $("#lk-img-del").addEventListener("click", () => commit({ image: "" }));

  $("#lk-presets").addEventListener("click", async (e) => {
    const t = T();
    const del = e.target.closest("[data-del]");
    if (del) {
      e.stopPropagation();
      const list = (settings().look_presets || []).filter((_, i) => i !== Number(del.dataset.del));
      if (await save({ look_presets: list }, true)) render();
      return;
    }
    if (e.target.closest("#lk-save")) {
      const box = $("#lk-presets");
      if (box.querySelector(".lk-name")) return;
      const inp = document.createElement("input");
      inp.className = "input input-sm lk-name";
      inp.placeholder = t.lk_name;
      box.insertBefore(inp, $("#lk-save"));
      inp.focus();
      const done = async (ok) => {
        const name = inp.value.trim();
        inp.remove();
        if (!ok || !name) return;
        const list = [...(settings().look_presets || []).filter((p) => p.name !== name), { name, look: look() }];
        if (await save({ look_presets: list }, true)) {
          toast(t.lk_saved);
          render();
        }
      };
      inp.addEventListener("keydown", (k) => {
        if (k.key === "Enter") done(true);
        if (k.key === "Escape") done(false);
      });
      inp.addEventListener("blur", () => done(true));
      return;
    }
    const chip = e.target.closest("[data-p]");
    if (!chip) return;
    const i = Number(chip.dataset.p);
    const p = i < 0 ? DEFAULT_LOOK : (settings().look_presets || [])[i]?.look;
    if (p) commit({ ...DEFAULT_LOOK, ...p });
  });

  return {
    init() {
      apply();
      render();
    },
    render() {
      apply();
      render();
    },
    apply,
  };
}
