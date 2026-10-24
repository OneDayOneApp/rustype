<p align="center"><img src="assets/logo.png" width="560" alt="Rustype"></p>

**English** | [简体中文](README.zh-CN.md)

# Rustype

A Rust reimplementation of a Typeless-style AI dictation app for macOS. Tap the right `Option` (⌥) key to start talking, tap again to stop — Rustype transcribes the audio, cleans it up, and writes the result into the focused input field.

The behavior is modeled on a reverse-engineering analysis of Typeless 2.8.2. The analysis documents are written in Chinese:

- [Reverse-engineering report](docs/REVERSE_ENGINEERING.md)
- [Decision log / thinking process](docs/THINKING_PROCESS.md)

## How it works

```
CGEventTap (⌥ / chords)
        │  tap right Option to start / stop
        ▼
Session state machine ── 16 kHz mono ──▶ cpal microphone capture
        │  trim silence + resample + WAV
        ▼
ASR (OpenAI-compatible /audio/transcriptions)
        │  dictionary + frontmost app / window / selected text
        ▼
LLM cleanup · rewrite · translate
        │  clipboard + Cmd+V (or synthesized Unicode typing)
        ▼
Focused input field
```

## Features

- **Tap-to-toggle** (default): tap the right `Option` key to start recording, tap again to stop and enter the *Thinking* state; `Esc` cancels.
- **Anti-hallucination guards**: silent or too-quiet audio is never sent to ASR; a transcript whose length is implausible for the recording duration (e.g. 850 characters from 4 seconds of silence) is discarded; when LLM cleanup inflates the text abnormally, Rustype falls back to the raw transcript.
- **Hold-to-talk**: switch *Trigger mode* to `hold` in the settings panel to talk while the key is down (double-tap hands-free is still supported).
- **Three modes**:
  - **Dictation** (right `Option`): removes filler words, resolves self-corrections, adds punctuation, paragraphs and lists.
  - **Ask / command** (right `Option` + `Space`): rewrites the selected text when there is a selection, otherwise answers the spoken question.
  - **Translation** (right `Option` + `Shift`): translates into the configured target language.
- **Context aware**: injects the frontmost app name, window title, selected text and a personal dictionary into the prompt.
- **Insertion**: clipboard + `Cmd+V` with clipboard restoration by default; optional Unicode synthesized typing.
- **Local history**: `history.jsonl`; `Ctrl+Cmd+V` pastes the previous result.
- **Configurable**: TOML config and any OpenAI-compatible endpoint (OpenAI, Azure gateways, local Whisper, Ollama/vLLM, and so on).

## Icon and logo

<p>
  <img src="assets/icon-1024.png" width="140" alt="Rustype icon">
  &nbsp;&nbsp;
  <img src="assets/mark-1024.png" width="140" alt="Rustype mark">
</p>

| File | Purpose |
| --- | --- |
| `assets/icon.svg` / `icon-1024.png` / `icon.icns` | App icon (Finder / menu metadata) |
| `assets/mark-1024.png` | Standalone mark (microphone + telegraph arcs) |
| `assets/logo.svg` / `logo.png` | Horizontal wordmark for light backgrounds |
| `assets/logo-dark.png` | Horizontal wordmark for dark backgrounds |

Design: a dark squircle with a white microphone and one thin rust-orange telegraph arc on each side (signal / incoming message), kept deliberately restrained; the wordmark sets "Rust" in ink and "ype" in rust orange. Regenerate everything with:

```bash
python3 scripts/make-icons.py   # requires Pillow; also builds icon.icns
```

## Install

### DMG

```bash
scripts/make-dmg.sh          # rebuild the app and create the disk image
open dist/Rustype-0.1.0.dmg  # drag Rustype.app into Applications
```

- arm64 only: `scripts/make-dmg.sh` → `dist/Rustype-0.1.0.dmg`
- Universal (arm64 + x86_64): `scripts/make-universal.sh` → `dist/Rustype-0.1.0-universal.dmg`

The image contains `Rustype.app`, an `Applications` shortcut and a short install note. If Gatekeeper blocks the first launch, right-click → **Open**; then enable Rustype under **System Settings → Privacy & Security → Accessibility**.

> The app is ad-hoc signed and not notarized. For distribution to other Macs, sign with a Developer ID certificate and notarize — see [docs/SIGNING.md](docs/SIGNING.md).

### Build from source

```bash
cargo build --release
# binary: target/release/rustype
```

## Permissions

Open **System Settings → Privacy & Security → Accessibility** and add the `rustype` executable (`Cmd+Shift+G` lets you paste the path directly).

The permission is used to:

- listen for global hotkeys,
- synthesize `Cmd+V` into other apps,
- read the selected text through the Accessibility API.

> When launched from a terminal, macOS attributes the Accessibility grant to the terminal (Terminal/iTerm). For a LaunchAgent daemon, grant it to the `rustype` binary itself. `rustype doctor` prints a reminder.

## Configuration

```bash
./target/release/rustype config --init    # ~/Library/Application Support/Rustype/config.toml
export RUSTYPE_API_KEY=sk-...             # or set provider.api_key in the config
./target/release/rustype doctor
```

> The default config lives in `~/Library/Application Support/Rustype/`. Set `RUSTYPE_HOME` to use another directory (the development setup in this repo uses `.rustype-home/`).

## Running

```bash
scripts/make-app.sh                             # build dist/Rustype.app
./dist/Rustype.app/Contents/MacOS/rustype run   # recommended: stable app identity
./target/release/rustype run --no-tray          # headless (ssh / launchd)
```

> macOS remembers the menu-bar position and visibility per *app identity*. Running the bare binary at `target/release/rustype` can leave the item hidden after a restart; the `.app` bundle uses `com.rustype.desktop`, which keeps the icon stable and makes the Accessibility entry show up as Rustype.

A microphone icon appears in the menu bar:

- **Click the icon** to open the menu: start/stop dictation, paste previous, settings…, open config file, quit.
- **Hold the right `Option` key** to talk; release to transcribe and insert (`●` next to the icon means recording, `…` means processing).
- The icon menu, the hotkey and the CLI all share the same session.

| Idle | Recording |
| --- | --- |
| ![idle](docs/menubar-idle.png) | ![recording](docs/menubar-recording.png) |

While recording, a pill appears at the bottom center of the screen: ✕ cancels on the left, the live waveform sits in the middle, and ✓ stops and transcribes on the right.

![recording pill](docs/recording-pill.png)

After pressing ✓ the pill switches to the processing state with a sweeping **Thinking** animation and disappears when transcription and cleanup finish (or fail):

![Thinking](docs/thinking-pill.png)

### Translation bar

In translation mode (right `Option` + `Shift`) a language bar appears above the pill, with 16 common languages:

![translation bar](docs/translation-bar.png)

- Picking a target language takes effect immediately and is written back to the config for the next translation.
- The bar only appears while recording in translation mode; dictation and command modes stay unobtrusive.

### Ask anything

In command mode (right `Option` + `Space`) a hint bar is shown above the pill while recording:

![ask bar](docs/ask-bar.png)

After stopping, the answer panel opens by default; only an explicit rewrite instruction with a selection replaces the selection.

### Answer card

Command mode opens a Typeless-style answer panel instead of replacing the selection:

![answer card](docs/answer-card.png)

- Header: `Rustype` title + close button.
- Question: microphone icon + what you just said + copy button.
- Answer: `回答` header + copy button + scrollable, selectable answer text.
- The panel follows the system appearance (light/dark).
- Only **a selection plus an explicit rewrite instruction** (rewrite / polish / translate / shorten…) replaces the selection.
- `rustype answer "text"` previews the card.

Context menu:

![menu](docs/menubar-menu.png)

### Settings panel

Menu-bar icon → **Settings…** opens a native settings panel. Saving hot-reloads the running daemon (models, proxy, hotkeys, microphone, insertion method) without a restart:

![settings panel](docs/settings-panel.png)

- API keys are shown in a secure field.
- Microphone and insertion method are pop-up menus.
- Hotkey syntax is validated before saving; invalid values show an alert and are not written.
- **Open config file** remains available for advanced options such as prompts.
- Saving from the tray hot-reloads immediately; `rustype settings` only writes the file (restart the daemon to apply).

### Launch at login (optional)

```bash
scripts/install-launchagent.sh          # install and start the LaunchAgent
scripts/install-launchagent.sh --remove # uninstall
```

## Hotkeys

| Action | Default | Notes |
| --- | --- | --- |
| Dictation | tap right `Option` (⌥) | tap again to stop and transcribe; toggle mode by default |
| Hold-to-talk | Settings → Trigger mode → hold | start on key down, stop on key up |
| Ask / rewrite selection | right `Option` + `Space` | rewrites a selection, or answers when there is none |
| Translate | right `Option` + `Shift` | target language from `translation_target` |
| Paste previous | `Ctrl+Alt+V` | reads from local history |
| Cancel recording | `Esc` | discards the current recording |

The right `Option` default avoids fighting Typeless for `Fn` (both use HID-level taps). To use `Fn` instead, quit Typeless first and change `[hotkeys]` back to `fn`.

All shortcuts are configurable in `[hotkeys]`, e.g. `fn+space` or `ctrl+alt+d`; chord character keys are swallowed by the event tap so they never leak into the focused app.

## Configuration reference

```toml
[provider]
base_url = "https://api.openai.com/v1"   # any OpenAI-compatible endpoint
api_key = ""                             # prefer the RUSTYPE_API_KEY env var
asr_model = "whisper-1"                  # or gpt-4o-transcribe / a local model name
llm_model = "gpt-4o-mini"
language = ""                            # e.g. "zh", "en"; empty = auto-detect
timeout_secs = 60
proxy = ""                               # e.g. "http://127.0.0.1:7890"; empty reads HTTPS_PROXY

[llm]
enabled = true                           # false = use the raw ASR transcript only
temperature = 0.2

[audio]
sample_rate = 16000
min_duration_ms = 300                    # drop recordings that are too short
max_duration_ms = 300000                 # auto-stop after 5 minutes
input_device = ""                        # name substring; empty = system default
silence_rms = 0.004                      # leading/trailing silence trim threshold
play_sounds = true
keep_mic_warm = true                     # keep the stream open to remove startup delay (mic indicator stays on)

[insert]
method = "paste"                         # paste | type
restore_clipboard = true
restore_delay_ms = 250

[hotkeys]
mode = "toggle"                          # toggle (tap) or hold (push-to-talk)
dictation = "right-alt"
command = "right-alt+space"
translation = "right-alt+shift"
paste_last = "ctrl+alt+v"

dictionary = ["Kubernetes", "Rustype"]   # personal dictionary injected into the prompt
translation_target = "English"
history_enabled = true
```

The three prompts under `[prompts]` can be replaced entirely to change the tone or to target a different model.

## Commands

| Command | Purpose |
| --- | --- |
| `rustype run` | persistent hotkey listener (default command) |
| `rustype transcribe <file.wav>` | transcribe a WAV offline (16-bit PCM / float32) and print it |
| `rustype devices` | list input devices |
| `rustype doctor` | check permissions, config, API key and devices |
| `rustype config [--init]` | print / create the config |
| `rustype insert "text"` | exercise the insertion path |
| `rustype settings` | open the settings panel standalone |
| `rustype answer "text"` | preview the answer card |
| `rustype askbar` | preview the "ask anything" hint bar |

## Tests

```bash
cargo test     # core unit tests: config, state machine, audio, prompts, client parsing
cargo build    # full macOS build
```

An end-to-end mock-provider test is available in the original development workspace (it uses the local `re/` reverse-engineering scripts, which are not part of this repository).

## Project layout

```
apps/rustype/             macOS executable
  src/macos/tap.rs        CGEventTap: Fn, chords, key swallowing
  src/macos/audio.rs      cpal capture
  src/macos/context.rs    NSWorkspace + Accessibility context
  src/macos/insert.rs     clipboard / Cmd+V / Unicode typing
  src/session.rs          record → transcribe → clean up → insert
crates/rustype-core/      platform-independent core
  config / hotkey / state / prompt / audio / asr / llm / history
docs/                     reverse-engineering report, decision log, screenshots, signing guide
scripts/                  packaging, icons, DMG, universal build, notarization
```

## Design choices and differences from Typeless

- **No reuse of Typeless cloud APIs**: `/ai/voice_flow` requires a Typeless account token and is a third-party service; Rustype uses a configurable OpenAI-compatible endpoint instead.
- **No full input-field capture**: Typeless uploads the text around the cursor and the whole input field for cleanup; Rustype only reads the frontmost app, window title and selected text — privacy first.
- **No local SQLite, cloud sync or accounts**: only a JSONL history file.
- **Different default trigger**: Typeless uses `Fn`; Rustype defaults to the right `Option` to avoid fighting a running Typeless for the same key. Both use HID-level taps; `Fn` can still be enabled in the config.

## Known limitations

- macOS only (Linux/Windows would require replacing `apps/rustype/src/macos`).
- The menu-bar icon is deliberately simple: click for the menu; there is no floating waveform strip like Typeless.
- `keep_mic_warm = true` keeps the audio stream open (the microphone indicator stays on) to remove startup latency; set it to `false` if that matters, at the cost of losing roughly the first 0.4 s of each recording.
- Opus upload is not built in: audio is sent as WAV. An `opus` feature could be added for endpoints that only accept Ogg/Opus (Typeless's 32 kbps parameters are recorded in the RE report).
- "Ask anything" external actions (open a link, create a document) and personalized statistics are not implemented.

## Signing

Local builds are ad-hoc signed. For a stable Accessibility grant across updates and for Gatekeeper-friendly distribution, use a Developer ID certificate and notarize the DMG. The complete procedure and scripts are in [docs/SIGNING.md](docs/SIGNING.md).

## Compliance

This project is for learning and interoperability research only: it contains no Typeless code or assets, does not call its private APIs and does not bypass its payment limits. The reverse-engineering analysis was performed on locally installed software artifacts; no network capture or account-data extraction was involved.
