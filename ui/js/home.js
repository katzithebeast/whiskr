// Home: intro animation, quick actions, "what's new".
import { cat, keyLabels } from "./cat.js";

export const CHANGELOG = [
  { v: "0.16.5", d: "2026-10-21",
    cs: ["Shift+Space: každý stisk otevře nebo zavře", "Po zavření chatu se klávesnice vrátí do okna, kde jsi byl — dřív zůstala na schovaném chatu a další Shift+Space nic neudělalo."],
    en: ["Shift+Space: every press opens or closes", "After the chat closes, the keyboard goes back to the window you were in — it used to stay on the hidden chat, so the next Shift+Space did nothing."] },
  { v: "0.16.4", d: "2026-10-21",
    cs: ["Vzorce v chatu", "Matematické vzorce v odpovědích ($…$, $$…$$, \\(…\\), \\[…\\]) se vykreslí jako opravdové vzorce, ne jako text."],
    en: ["Formulas in the chat", "Math in answers ($…$, $$…$$, \\(…\\), \\[…\\]) renders as real formulas, not plain text."] },
  { v: "0.16.3", d: "2026-10-21",
    cs: ["Plynulé zavírání chatu", "Chat se zavře hned, i když zrovna přichází odpověď (ta se dopíše na pozadí). Opravené několikasekundové zaseknutí při otevírání, kvůli kterému se zavírání zpožďovalo."],
    en: ["Smooth chat closing", "The chat closes right away even while an answer is streaming in (it keeps arriving in the background). The big chat's closing animation is shorter and smoother."] },
  { v: "0.16", d: "2026-10-21",
    cs: ["Instalačka a aktualizace jedním klikem", "Whiskr má instalačku a sám se podívá po nové verzi. Když je, na úvodní stránce se objeví Aktualizovat — stáhne, nainstaluje a znovu spustí."],
    en: ["Installer and one-click updates", "Whiskr has an installer and checks for new versions itself. When there is one, Update appears on the home page — it downloads, installs and restarts."] },
  { v: "0.15.2", d: "2026-10-21",
    cs: ["Zámek zkratek na hraní", "Ctrl+Alt+Shift+L, ikona zámku v bočním panelu nebo položka v liště zamkne všechny zkratky Whiskru — klávesy jdou rovnou do hry. Ve hrách ze seznamu (výchozí League of Legends / TFT) se zkratky pauznou samy."],
    en: ["Shortcut lock for gaming", "Ctrl+Alt+Shift+L, the lock icon in the sidebar or the tray item locks all Whiskr shortcuts — keys go straight to the game. In games from your list (League of Legends / TFT by default) shortcuts pause by themselves."] },
  { v: "0.15.1", d: "2026-10-21",
    cs: ["Plynulé Shift+Space", "Držení Shift+Space už chat neotvírá a nezavírá dokola. Zavírání je rychlé a další stisk platí hned. Whiskr si sám hlídá, jestli držíš Shift."],
    en: ["Smooth Shift+Space", "Holding Shift+Space no longer toggles the chat over and over. Closing is quick and the next press counts right away. Whiskr tracks Shift itself."] },
  { v: "0.15", d: "2026-10-21",
    cs: ["Mluv rovnou do chatu", "Podrž Shift+Backspace — vyjede chat a poslouchá. Po puštění se řeč přepíše a otázka rovnou odešle. Krátké ťuknutí chat zavře (jinde funguje jako obyčejný Backspace). Opravené držení kláves pro lidi s dlouhou prodlevou opakování."],
    en: ["Talk straight into the chat", "Hold Shift+Backspace — the chat slides up and listens. On release your words are transcribed and sent. A quick tap closes the chat (elsewhere it's a normal Backspace). Fixed key holds for long key-repeat delays."] },
  { v: "0.14.10", d: "2026-10-21",
    cs: ["Hladké rohy chatu", "Rohy rychlého chatu jsou zase vyhlazené."],
    en: ["Smooth chat corners", "The quick chat's corners are smooth again."] },
  { v: "0.14.9", d: "2026-10-21",
    cs: ["Žádné prosvítání tapety", "Okraj hlavního okna i chatu byl z velké části průhledný a světlá tapeta jím prosvítala jako záře nahoře. Teď je okraj plný."],
    en: ["No more wallpaper glow", "The edge of the main window and the chat was mostly see-through, so a bright wallpaper glowed along the top. The edge is solid now."] },
  { v: "0.14.8", d: "2026-10-21",
    cs: ["Obrázky z Codexu ve WSL a čistý rámeček chatu", "Obrázky, které Codex vygeneruje ve WSL, se zkopírují k Whiskru a ukážou v chatu. Rychlý chat už nemá světlou linku nahoře."],
    en: ["Codex images from WSL and a clean chat frame", "Pictures Codex makes in WSL are copied over and shown in the chat. The quick chat no longer has a light line at the top."] },
  { v: "0.14.7", d: "2026-10-21",
    cs: ["Shift+Space opravdu spolehlivě", "Whiskr omylem považoval některá okna za hru na celou obrazovku a Shift+Space v nich ignoroval. Chat se už sám nezavře hned po otevření a chat v hlavním okně zase dostává odpovědi."],
    en: ["Shift+Space, reliably", "Whiskr mistook some windows for a fullscreen game and ignored Shift+Space there. The chat no longer closes itself right after opening, and the chat in the main window gets its answers again."] },
  { v: "0.14.6", d: "2026-10-21",
    cs: ["Obrázky od Codexu a diagnostika", "Obrázky, které Codex vygeneruje, se ukážou přímo v chatu (klikem otevřeš). Při dlouhém mlčení agenta chat ukáže, že pořád pracuje. Nový diagnostický log whiskr-debug.log vedle aplikace."],
    en: ["Codex images and diagnostics", "Pictures Codex generates show right in the chat (click to open). A long-silent agent now shows it's still working. New diagnostic log whiskr-debug.log next to the app."] },
  { v: "0.14.5", d: "2026-10-21",
    cs: ["Znovuotevření rychlého chatu", "Třetí a každé další Shift+Space chat spolehlivě otevře. Opožděné zavření z minula už nově otevřený chat neschová."],
    en: ["Reopening the quick chat", "The third and every further Shift+Space reliably opens the chat. A late close from before no longer hides a fresh opening."] },
  { v: "0.14.4", d: "2026-10-21",
    cs: ["Opravený Shift+Space a seznam chatů", "Whiskr už při otevření chatu nepouští Shift za tebe, takže další Shift+Space spolehlivě zavře. Seznam chatů v hlavním okně se načte hned a postranní panely startují zatažené."],
    en: ["Fixed Shift+Space and the chat list", "Whiskr no longer releases Shift for you when the chat opens, so the next Shift+Space reliably closes it. The chat list loads right away and sidebars start folded."] },
  { v: "0.14.3", d: "2026-10-21",
    cs: ["Spolehlivý Shift+Space a nová verze bez přepínání", "Shift+Space už nespolkne stisk po ztraceném puštění klávesy, rychlý chat se poprvé otevře bez probliknutí a nově spuštěná verze sama nahradí tu, co běží v liště."],
    en: ["Reliable Shift+Space and seamless updates", "Shift+Space no longer swallows a press after a lost key release, the quick chat opens without a flash the first time, and a newly started build replaces the one in the tray."] },
  { v: "0.14.2", d: "2026-10-21",
    cs: ["Jeden postranní panel a spolehlivější rychlý chat", "Na stránce Chat se levý panel promění na seznam chatů a zpátky. Rychlý chat se otevírá bez probliknutí a znovu otevření spolehlivě převezme klávesnici."],
    en: ["One sidebar and a more reliable quick chat", "On the Chat page the sidebar turns into your chat list and back. The quick chat opens without a flash and reopening reliably takes the keyboard."] },
  { v: "0.14.1", d: "2026-10-21",
    cs: ["Chat přímo v aplikaci a vlastní pozadí úvodu", "Chat v bočním panelu se otevře uvnitř hlavního okna a zpátky se vrátíš jedním klikem. Obrázek na úvodní stránce změníš v Nastavení. Zmizela světlá linka kolem oken."],
    en: ["Chat inside the app and your own home picture", "Chat from the sidebar opens inside the main window. Change the home picture in Settings. The light edge around windows is gone."] },
  { v: "0.14", d: "2026-10-21",
    cs: ["Nová úvodní stránka a hladký start", "Úvodní stránka s horami a třemi hlavními přednostmi. Okno se ukáže až s hotovou úvodní animací a chat se při zavření plynule sbalí zpátky."],
    en: ["New home and a smoother start", "A mountain hero with three highlights. The window shows only once the intro is ready, and the chat now folds away smoothly when closed."] },
  { v: "0.13", d: "2026-10-21",
    cs: ["Nová domovská stránka a navigace", "Domovská stránka ve stylu promo stránky s časovou osou novinek. Boční panel je zatažený a vytáhneš ho jedním klikem, mezi chatem a dashboardem přepneš z bočního panelu."],
    en: ["New home page and navigation", "A promo-style home with a timeline of what's new. The sidebar folds away, and you switch between chat and dashboard from it."] },
  { v: "0.12", d: "2026-10-21",
    cs: ["Úvodní animace s kočkou", "Kočka přivítá při spuštění Whiskru. Chat se rozbaluje plynule a bez problikávání."],
    en: ["Intro animation", "The cat says hi when Whiskr starts. The chat expands smoothly without flicker."] },
  { v: "0.11", d: "2026-10-21",
    cs: ["Pozadí, matné sklo a rychlé odpovědi", "Vlastní barva nebo obrázek, rozmazání, průhlednost a presety. Claude odpovídá rychleji bez startu konektorů."],
    en: ["Backgrounds, frosted glass and fast answers", "Own colour or image, blur, opacity and presets. Claude answers sooner by skipping connector start-up."] },
  { v: "0.10", d: "2026-10-21",
    cs: ["Výběr modelů a chytřejší chat", "Opus, Sonnet, GPT a další přímo v chatu, Shift+Space+R pro nový chat, vypisování znak po znaku."],
    en: ["Model picker and a smarter chat", "Opus, Sonnet, GPT and more right in the chat, Shift+Space+R for a new chat, typewriter answers."] },
  { v: "0.9", d: "2026-10-21",
    cs: ["Přehledné nastavení a připojení", "Nastavení v záložkách. Účty Claude, Codex a Cursor a MCP konektory na jedno kliknutí."],
    en: ["Tidy settings and connections", "Tabbed settings. Claude, Codex and Cursor accounts and one-click MCP connectors."] },
  { v: "0.8", d: "2026-10-21",
    cs: ["Tmavý vzhled", "Tmavé téma a vlastní okno bez systémového rámu."],
    en: ["Dark look", "Dark theme and a custom window without the system frame."] },
  { v: "0.7", d: "2026-10-21",
    cs: ["Asistent pro každý den", "Úkoly a připomínky hlasem, ranní briefing, brief před schůzkou a poznání hovoru."],
    en: ["An everyday assistant", "Tasks and reminders by voice, morning briefing, meeting brief and call detection."] },
  { v: "0.6", d: "2026-10-21",
    cs: ["Whiskr Ask", "Shift+Space a ptej se Clauda nebo GPT, nech je debatovat nebo deleguj práci agentům."],
    en: ["Whiskr Ask", "Shift+Space to ask Claude or GPT, let them debate or delegate work to agents."] },
];

export function initHome(ctx) {
  const { invoke, esc, $, T, settings, go, save, toast } = ctx;
  const lang = () => (settings().ui_lang === "en" ? "en" : "cs");
  let newsOpen = false;

  // ───────── quick actions ─────────
  const ICONS = {
    dictate: '<svg viewBox="0 0 24 24"><rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21"/></svg>',
    ask: '<svg viewBox="0 0 24 24"><path d="M5 5.5h14v10H10l-5 4z"/></svg>',
    task: '<svg viewBox="0 0 24 24"><rect x="4" y="4" width="16" height="16" rx="4"/><path d="M8.5 12.5l2.5 2.5 4.5-5"/></svg>',
    record: '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="8"/><circle cx="12" cy="12" r="3.2" fill="currentColor"/></svg>',
  };
  function renderQuick() {
    const t = T();
    const s = settings();
    const items = [
      ["dictate", s.hotkey, t.q_dictate, t.q_dictate_sub],
      ["ask", s.hotkey_ask, t.q_ask, t.q_ask_sub],
      ["task", s.hotkey_task, t.q_task, t.q_task_sub],
      ["record", s.hotkey_record, t.q_record, t.q_record_sub],
    ];
    $("#quick").innerHTML = items
      .map(
        ([k, spec, name, sub]) => `<button class="q" data-q="${k}">
          <span class="qi">${ICONS[k]}</span>
          <b>${esc(name)}</b><span class="qs">${esc(sub)}</span>
          <span class="qk">${spec ? keyLabels(spec).map((x) => `<kbd>${esc(x)}</kbd>`).join("") : ""}</span>
        </button>`
      )
      .join("");
    $("#home-ask-key").textContent = s.hotkey_ask ? keyLabels(s.hotkey_ask).join("+") : "";
  }
  $("#quick").addEventListener("click", (e) => {
    const b = e.target.closest("[data-q]");
    if (!b) return;
    const k = b.dataset.q;
    if (k === "ask") invoke("ask_open_with", { text: "", send: false });
    if (k === "task") go("today");
    if (k === "record") go("recordings");
    if (k === "dictate") go("history");
  });
  $("#home-ask").addEventListener("click", () => invoke("ask_open_with", { text: "", send: false }));

  // ───────── what's new (timeline) ─────────
  function renderNews() {
    const t = T();
    const list = newsOpen ? CHANGELOG : CHANGELOG.slice(0, 4);
    const fmt = (d) => new Date(d + "T12:00:00").toLocaleDateString(lang(), { day: "numeric", month: "long", year: "numeric" });
    $("#news").innerHTML =
      list
        .map(
          (c, i) => `<article class="clog-item${i === 0 ? " latest" : ""}">
            <span class="clog-dot"></span>
            <div class="clog-meta"><svg viewBox="0 0 24 24"><rect x="4" y="5" width="16" height="15" rx="2.5"/><path d="M4 9.5h16M8.5 3.5v3M15.5 3.5v3"/></svg>${esc(fmt(c.d))}<span class="clog-v">v${esc(c.v)}</span></div>
            <h3>${esc(c[lang()][0])}</h3>
            <p>${esc(c[lang()][1])}</p>
          </article>`
        )
        .join("") +
      (CHANGELOG.length > 4 ? `<button class="btn btn-ghost btn-sm clog-more" id="news-toggle">${esc(newsOpen ? t.news_less : t.news_more)}</button>` : "");
    const tg = $("#news-toggle");
    if (tg)
      tg.onclick = () => {
        newsOpen = !newsOpen;
        renderNews();
      };
  }

  // ───────── intro animation ─────────
  // The overlay is on screen from the first paint; it starts once visible.
  let played = false;
  const splash = $("#splash");
  $("#sp-cat").innerHTML = cat({ body: "var(--black)", eyes: "var(--cream)" });
  function finishSplash(fast) {
    if (splash.hidden || splash.classList.contains("out")) return;
    if (fast) {
      splash.hidden = true;
      document.body.classList.remove("splashing");
      return;
    }
    splash.classList.add("out");
    document.body.classList.remove("splashing");
    document.body.classList.add("arrive");
    setTimeout(() => {
      splash.hidden = true;
      splash.classList.remove("out", "play");
      document.body.classList.remove("arrive");
    }, 720);
  }
  function playSplash() {
    if (played) return;
    played = true;
    if (settings().splash === false) return finishSplash(true);
    splash.classList.add("play");
    splash.onclick = () => finishSplash(false);
    setTimeout(() => finishSplash(false), 3300);
  }
  function maybeSplash() {
    if (document.visibilityState === "visible") playSplash();
  }
  document.addEventListener("visibilitychange", maybeSplash);
  ctx.listen?.("whiskr:main-shown", () => playSplash());

  $("#news-cue")?.addEventListener("click", () => $("#clog")?.scrollIntoView({ behavior: "smooth", block: "start" }));
  // ───────── hero picture (Settings → Home page) ─────────
  const TAURI = window.__TAURI__;
  function applyHomeBg() {
    const v = settings().home_bg || "";
    const bg = document.querySelector(".promo-bg");
    const th = $("#hb-thumb");
    if (!bg) return;
    if (v === "none") {
      bg.style.backgroundImage = "none";
      if (th) { th.style.backgroundImage = "none"; th.classList.add("empty"); }
    } else if (v) {
      const src = TAURI?.core?.convertFileSrc ? TAURI.core.convertFileSrc(v) : v;
      bg.style.backgroundImage = `url("${src}")`;
      if (th) { th.style.backgroundImage = `url("${src}")`; th.classList.remove("empty"); }
    } else {
      bg.style.backgroundImage = "";
      if (th) { th.style.backgroundImage = 'url("img/hero.svg")'; th.classList.remove("empty"); }
    }
  }
  $("#hb-pick")?.addEventListener("click", async () => {
    try {
      const path = await invoke("pick_background");
      if (path && (await save({ home_bg: path }, true))) applyHomeBg();
    } catch (e) { toast?.(String(e)); }
  });
  $("#hb-default")?.addEventListener("click", async () => { if (await save({ home_bg: "" }, true)) applyHomeBg(); });
  $("#hb-none")?.addEventListener("click", async () => { if (await save({ home_bg: "none" }, true)) applyHomeBg(); });

  window.__whiskrHome = true;
  return {
    init() {
      $("#hero-cat").innerHTML = cat({ body: "var(--black)", eyes: "var(--cream)", cls: "hero-cat" });
      renderQuick();
      renderNews();
      applyHomeBg();
      maybeSplash();
      // tell Rust the intro overlay is painted, so it can show the window now
      setTimeout(() => invoke("main_ready").catch(() => {}), 80); // timers (not rAF) still run while the window is hidden
    },
    render() {
      renderQuick();
      renderNews();
      applyHomeBg();
    },
  };
}
