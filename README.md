# Whiskr 🐱

**Tvůj hlas. Tvůj flow.** Diktování do jakékoli aplikace, nahrávání schůzek a rychlý chat s Claudem nebo GPT — jednou zkratkou, česky a z tvého vlastního předplatného. Zatím pro Windows.

## Instalace

1. Stáhni nejnovější **`Whiskr_x.y.z_x64-setup.exe`** z [Releases](../../releases/latest) a spusť ho (bez admin práv).
   Windows může u nepodepsané appky ukázat „Systém Windows ochránil váš počítač“ → **Další informace → Přesto spustit**.
2. Na <https://console.groq.com/keys> si zdarma vytvoř API klíč (bez platební karty) a vlož ho do Whiskru.
3. Hotovo: podrž **Ctrl + Shift + Space**, mluv, pusť.

**Aktualizace:** Whiskr se sám podívá, jestli je nová verze. Když je, na úvodní stránce se objeví **Aktualizovat**. Jedno kliknutí ji stáhne, nainstaluje a Whiskr se znovu spustí. Ručně: Nastavení → Obecné → Aktualizace → Zkontrolovat.

## Zkratky

| Zkratka | Co dělá |
|---|---|
| **Ctrl+Shift+Space** | Diktování — podrž a mluv. Krátké ťuknutí = hands-free, ťukni znovu pro konec. |
| **Shift+Space** | Rychlý chat (Claude / GPT / Cursor). Znovu = zavřít. |
| **Shift+Space+R** | Úplně nový chat. |
| **Podrž Shift+Backspace** | Mluv rovnou do chatu — po puštění se otázka odešle. Ťuknutí = zavřít chat. |
| **Alt+Shift+T** | Úkol / připomínka hlasem („zítra v 9 zavolat Chrisovi“). |
| **Alt+Shift+E** | Command mode — označ text a řekni, co s ním. |
| **Alt+Shift+R** | Nahrávání schůzky (start / stop). |
| **Alt+Shift+B** | Záložka v nahrávce. |
| **Alt+Shift+V** | Vložit poslední diktát znovu. |
| **Ctrl+Alt+Shift+L** | Zamknout / odemknout zkratky (hraní). Ve hrách ze seznamu se zamykají samy. |
| **Esc** | Zrušit nahrávání. |

Všechny zkratky jdou změnit v Nastavení → Zkratky.

## Co umí

- **Diktování** přes Groq Whisper s úklidem textu (bez „ehm“, opravy, interpunkce), vlastní slovník.
- **Rychlý chat** s Claude Code, Codexem a Cursor Agentem z tvých předplatných — ptej se, nech je debatovat, nebo deleguj práci v projektu. Obrázky z Codexu přímo v chatu.
- **Schůzky**: pozná hovor (Teams, Meet, Zoom…), nahraje i zvuk počítače, přepis, shrnutí, úkoly.
- **Asistent**: úkoly a připomínky hlasem, ranní briefing, brief před schůzkou.
- **MCP konektory** pro Claude, Codex, Cursor i Claude Desktop.

Nastavení, klíč a historie zůstávají jen v počítači (`%APPDATA%\app.whiskr.desktop`).

## Vývoj

```
ui/                    frontend (čisté HTML/CSS/JS, bez bundleru)
src-tauri/src/         jádro v Rustu (Tauri 2)
.github/workflows/     release.yml — sestaví instalačku a vydá ji
```

```bash
npm install
npx tauri dev          # vývoj
npx tauri build        # instalačka v src-tauri/target/release/bundle/nsis/
```

**Vydání nové verze:** zvyš `version` v `src-tauri/Cargo.toml` i `src-tauri/tauri.conf.json` a pushni na `main`:

```bash
git commit -am "Whiskr 0.16.1" && git push
```

GitHub Actions pozná novou verzi, sestaví instalačku a vydá ji jako Release `v0.16.1`. Aplikace u všech si ji samy najdou.
