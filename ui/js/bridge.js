// Thin wrapper over the Tauri globals. Outside Tauri (plain browser preview)
// it falls back to an in-memory mock so the UI can be designed and tested.

const T = window.__TAURI__;

const DEMO_RECS = [
  { id: "20261020-150000", title: "Call s Acme — napojení e-mailů", created: new Date(Date.now() - 2 * 3600e3).toISOString(), seconds: 1260, has_system: true,
    status: "done", stage: "", engine: "whiskr-voices", error: null,
    speakers: { ME: "Já", A: "Jana", B: "Mluvčí B" },
    utterances: [
      { speaker: "ME", start: 3, end: 8, text: "Ahoj Jano, jak jste zatím spokojení s chatbotem?" },
      { speaker: "A", start: 9, end: 21, text: "Dobře, zákazníci ho používají hodně. Jen bychom chtěli, aby odpovídal i na e-maily, ideálně do konce měsíce." },
      { speaker: "B", start: 22, end: 30, text: "A potřebujeme, aby se citlivé dotazy předávaly operátorovi." },
      { speaker: "ME", start: 31, end: 40, text: "Jasně. Pošlu vám do pátku návrh, jak to napojit, a předávání operátorovi tam zahrnu." },
    ],
    summary: { overview: "Klient je s chatbotem spokojený a chce ho rozšířit o odpovídání na e-maily do konce měsíce, s předáváním citlivých dotazů operátorovi.",
      action_items: ["Filip — poslat návrh napojení e-mailů do pátku", "Jana — poslat seznam citlivých témat"], decisions: ["Chatbot se rozšíří o e-maily", "Citlivé dotazy půjdou operátorovi"], topics: ["chatbot", "e-maily", "Acme"],
      template: "sales",
      sections: [
        { title: "Potřeby klienta", items: ["Odpovídání na e-maily zákazníků", "Předávání citlivých dotazů živému operátorovi"] },
        { title: "Rozpočet a termíny", items: ["Spuštění ideálně do konce měsíce"] },
        { title: "Další kroky", items: ["Návrh napojení do pátku", "Seznam citlivých témat od Jany"] },
      ],
      highlights: [{ time: 22, text: "Citlivé dotazy musí jít operátorovi — klíčový požadavek" }] },
    template: "auto", bookmarks: [24] },
  { id: "20261020-100000", title: "Nahrávka 20. 10. 10:00", created: new Date(Date.now() - 6 * 3600e3).toISOString(), seconds: 2400, has_system: false,
    status: "processing", stage: "speakers", engine: "", error: null, speakers: {}, utterances: [], summary: null },
];

function silentWav(sec) {
  const rate = 8000, n = rate * sec, buf = new ArrayBuffer(44 + n * 2), v = new DataView(buf);
  const w = (o, str) => [...str].forEach((c, i) => v.setUint8(o + i, c.charCodeAt(0)));
  w(0, "RIFF"); v.setUint32(4, 36 + n * 2, true); w(8, "WAVE"); w(12, "fmt "); v.setUint32(16, 16, true);
  v.setUint16(20, 1, true); v.setUint16(22, 1, true); v.setUint32(24, rate, true); v.setUint32(28, rate * 2, true);
  v.setUint16(32, 2, true); v.setUint16(34, 16, true); w(36, "data"); v.setUint32(40, n * 2, true);
  return URL.createObjectURL(new Blob([buf], { type: "audio/wav" }));
}

function mock() {
  const state = {
    settings: {
      api_key: location.hash.includes("demo") ? "gsk_demo" : "", hotkey: "ctrl+shift+Space", hotkey_command: "alt+shift+KeyE", hotkey_record: "alt+shift+KeyR", hotkey_paste: "alt+shift+KeyV", hotkey_bookmark: "alt+shift+KeyB", default_template: "auto",
      custom_templates: [{ id: "custom-1", name: "Sprint review", description: "", instructions: "Sekce: Hotovo, Nehotovo, Rizika" }],
      hotkey_ask: "shift+Space", hotkey_task: "alt+shift+KeyT", theme: "dark", look: { color: "", image: "", blur: 0, dim: 35, opacity: 100 }, look_presets: [{ name: "Noc", look: { color: "#101826", image: "", blur: 0, dim: 35, opacity: 92 } }], ask_fast: true, ask_space_new: false, mcp_library: [{ name: "github", kind: "stdio", command: "npx", args: ["-y", "@modelcontextprotocol/server-github"], url: "", env: ["GITHUB_TOKEN=ghp_xxx"] }], brain: "auto", brain_profile: "work", briefing: true, briefing_time: "08:00", briefing_open: true, briefing_all_tools: true, briefing_tools: [], briefing_extra: "", meeting_detect: true, meeting_autostop: true, ask_engine: "claude", ask_runtime: "auto", ask_access: "safe", ask_selection: true, active_profile: "personal",
      profiles: [{ id: "personal", name: "Osobní", key: "" }, { id: "work", name: "Práce", key: "work" }],
      projects: [{ name: "iris", path: "C:\\Users\\user\\code\\iris", profile: "personal" }, { name: "acme-web", path: "C:\\Users\\user\\work\\acme", profile: "work" }], app_tone: true, language: "cs", polish: "light",
      dictionary: ["Whiskr", "Iris", "Hermes"], sounds: true, autostart: false,
      microphone: null, ui_lang: "cs", bar_style: "classic", rec_system_audio: true,
    },
    paused: false,
    history: {
      total_words: 1284, total_seconds: 610,
      items: [
        { id: 3, at: new Date().toISOString(), text: "Ahoj Petrau, posílám ti finální verzi nabídky. Mrkni na to prosím do čtvrtka.", raw: "ehm ahoj patriku posílám ti finální verzi nabídky mrkni na to prosím do středy vlastně do čtvrtka", seconds: 6.2, words: 13 },
        { id: 2, at: new Date(Date.now() - 3600e3).toISOString(), text: "Oprav v komponentě Header ten bug s přetékajícím menu na mobilu a přidej k tomu test.", raw: "oprav v komponentě header ten bug s přetékajícím menu na mobilu a přidej k tomu test", seconds: 5.1, words: 16 },
        { id: 1, at: new Date(Date.now() - 86400e3).toISOString(), text: "Meeting notes:\n- dokončit feature do konce měsíce\n- zpětná vazba od klienta k UI", raw: "meeting notes dokončit feature do konce měsíce a zpětná vazba od klienta k UI", seconds: 7.4, words: 14 },
      ],
    },
  };
  const handlers = {
    get_settings: () => state.settings,
    save_settings: ({ settings }) => (state.settings = settings),
    check_key: ({ key }) => (key.startsWith("gsk_") ? null : Promise.reject("Neplatný API klíč — zkontroluj ho v nastavení")),
    get_history: () => state.history,
    delete_entry: () => null,
    clear_history: () => null,
    copy_text: () => null,
    list_mics: () => ["Mikrofon (Realtek Audio)", "Headset (AirPods Pro)"],
    get_paused: () => state.paused,
    toggle_paused: () => (state.paused = !state.paused),
    suspend_hotkey: () => null,
    preview_bar: () => null,
    meeting_status: () => ({ recording: false, seconds: 0, has_system: false }),
    start_meeting: () => ({ recording: true, seconds: 0, has_system: true }),
    stop_meeting: () => null,
    list_recordings: () => DEMO_RECS,
    get_recording: ({ id }) => DEMO_RECS.find((r) => r.id === id),
    rename_speaker: ({ id, label, name }) => { const r = DEMO_RECS.find((x) => x.id === id); r.speakers[label] = name; return r; },
    rename_recording: () => null, reprocess_recording: () => null, delete_recording: () => null,
    recording_markdown: () => "# demo", open_recording_folder: () => null,
    list_voices: () => ({ model_ready: true, me: 4, voices: [{ name: "Jana", recordings: 2 }, { name: "Petra", recordings: 5 }] }),
    forget_voice: () => null,
    mcp_info: () => ({ claude_code_cmd: "claude mcp add whiskr -- whiskr-mcp.exe", desktop_json: "{}" }),
    connect_claude_desktop: () => ["claude_desktop_config.json"],
    app_version: () => "0.5.0",
    list_templates: () => [
      ["meeting", "Porada"], ["sales", "Obchodní call"], ["oneonone", "1:1"], ["interview", "Pohovor"],
      ["lecture", "Přednáška / škola"], ["brainstorm", "Brainstorming"], ["notes", "Hlasová poznámka"],
    ].map(([id, name]) => ({ id, name, description: "", instructions: "", custom: false }))
      .concat((state.settings.custom_templates || []).map((c) => ({ ...c, custom: true }))),
    recording_audio: () => silentWav(45),
    search_recordings: ({ query }) => {
      const q = query.toLowerCase().slice(0, 4);
      return DEMO_RECS.flatMap((r) => r.utterances.filter((u) => u.text.toLowerCase().includes(q))
        .map((u) => ({ id: r.id, title: r.title, created: r.created, time: u.start, speaker: r.speakers[u.speaker], snippet: u.text, score: 1 })));
    },
    ask_recordings: async ({ id }) => {
      await new Promise((r) => setTimeout(r, 700));
      const rid = id || "20261020-150000";
      return `Dohodli jste se na dvou věcech:\n- Chatbot bude **odpovídat i na e-maily**, ideálně do konce měsíce [[${rid}@00:09]]\n- Citlivé dotazy se budou předávat operátorovi [[${rid}@00:22]]\n\nFilip pošle návrh napojení do pátku [[${rid}@00:31]].`;
    },
    resummarize: () => null,
    update_utterance: ({ id, index, text, speaker }) => {
      const r = DEMO_RECS.find((x) => x.id === id);
      if (text != null) r.utterances[index].text = text;
      if (speaker) r.utterances[index].speaker = speaker === "__new__" ? "C" : speaker;
      return r;
    },
    export_docx: () => "C:/demo.docx",
    add_bookmark: () => 42,
    ask_tools: () => ({ tools: [{ engine: "claude", runtime: "windows", path: "C:/Users/me/.local/bin/claude.exe", logged_in: true }, { engine: "claude", runtime: "wsl", path: "/home/me/.local/bin/claude", logged_in: null }, { engine: "codex", runtime: "windows", path: "C:/Users/me/AppData/Roaming/npm/codex.cmd", logged_in: false }], groq: true }),
    pick_folder: () => "C:\\Users\\user\\code\\whiskr",
    ask_login: () => null,
    set_window_theme: () => null,
    pick_background: () => null,
    set_glass: () => null,
    open_main: () => null,
    connections_status: () => new Promise((r) => setTimeout(() => r({ groq: true, accounts: [
      { engine: "claude", tools: [{ engine: "claude", runtime: "windows", path: "C:/x/claude.exe" }], logins: { personal: false, work: true } },
      { engine: "codex", tools: [{ engine: "codex", runtime: "windows", path: "C:/x/codex.cmd" }, { engine: "codex", runtime: "wsl", path: "/usr/bin/codex" }], logins: { personal: true, work: null } },
      { engine: "cursor", tools: [], logins: {} } ] }), 400)),
    mcp_status: () => new Promise((r) => setTimeout(() => r({
      targets: { claude: { available: true, installed: ["whiskr", "github"], note: "" }, codex: { available: true, installed: [], note: "" }, cursor: { available: true, installed: ["github"], note: "" }, desktop: { available: true, installed: ["whiskr", "filesystem"], note: "" } },
      found: [{ name: "filesystem", kind: "stdio", command: "npx", args: ["-y", "@modelcontextprotocol/server-filesystem", "C:/Users/user/Documents"], url: "", env: [] }],
      claude_ai: ["claude.ai Gmail", "claude.ai Google Calendar", "claude.ai Slack"],
      whiskr: { name: "whiskr", kind: "stdio", command: "C:/Users/user/AppData/Local/Whiskr/whiskr-mcp.exe", args: [], url: "", env: [] } }), 500)),
    mcp_install: () => new Promise((r) => setTimeout(r, 600)),
    account_logout: () => null,
    brain_status: () => ({ engine: "Claude", profile: "Práce", fallback: true }),
    brain_connectors: () => ["claude.ai Gmail", "claude.ai Google Calendar", "claude.ai Slack", "iris"],
    list_tasks: () => {
      const d = (h, m = 0, days = 0) => { const x = new Date(); x.setDate(x.getDate() + days); x.setHours(h, m, 0, 0); return x.toISOString(); };
      const now = new Date().toISOString();
      return [
        { id: "1", text: "Poslat Petraovi nabídku pro Acme", done: false, created: now, due: d(9, 0, -1), has_time: true, notified: true, source: "voice", said: "včera v 9 poslat Petraovi nabídku" },
        { id: "2", text: "Zavolat Chrisovi kvůli lokální AI", done: false, created: now, due: d(14, 30), has_time: true, notified: false, source: "voice", said: "dnes ve 14:30 zavolat Chrisovi" },
        { id: "3", text: "Dodat Acme návrh napojení e-mailů", done: false, created: now, due: d(9, 0, 3), has_time: false, notified: false, source: "meeting", source_id: "x" },
        { id: "4", text: "Projít checklist pro instalaci Claude u klienta", done: false, created: now, due: null, has_time: false, notified: false, source: "manual" },
        { id: "5", text: "Objednat školení pro Acme tým", done: true, created: now, done_at: now, due: null, source: "voice" },
      ];
    },
    get_briefing: () => ({ date: (() => { const x = new Date(); return `${x.getFullYear()}-${String(x.getMonth() + 1).padStart(2, "0")}-${String(x.getDate()).padStart(2, "0")}`; })(), generated: new Date(new Date().setHours(8, 2)).toISOString(), engine: "Claude",
      text: "**Dnes** — klidnější den: dvě schůzky a jeden úkol po termínu, který stojí za to dotáhnout hned ráno.\n\n### 📅 Program\n- **10:00** Weekly s Petraem (Teams)\n- **14:30** Call s Chrisem — lokální AI pro Vojtu\n\n### ✅ Úkoly\n- ⚠ Poslat Petraovi nabídku pro Acme *(od včera)*\n- Zavolat Chrisovi (14:30)\n\n### 📌 Navazuje na včerejšek\n- Acme čeká na návrh napojení e-mailů — slíbil jsi do pátku.\n\n### 💡 Tip\nNabídku pošli před weekly — Petra se na ni nejspíš zeptá." }),
    add_task: () => ({}), update_task: () => null, delete_task: () => null, refresh_briefing: () => null, ask_open_with: () => null,
  };
  return {
    invoke: async (cmd, args = {}) => {
      await new Promise((r) => setTimeout(r, 60));
      return handlers[cmd] ? handlers[cmd](args) : null;
    },
    listen: async () => () => {},
    openUrl: (url) => window.open(url, "_blank"),
  };
}

export const tauri = T
  ? {
      invoke: T.core.invoke,
      listen: T.event.listen,
      openUrl: (url) => (T.opener?.openUrl ? T.opener.openUrl(url) : T.core.invoke("plugin:opener|open_url", { url })),
    }
  : mock();
