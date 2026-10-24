# Rustype 0.1.0

A Rust reimplementation of a Typeless-style AI dictation app for macOS.

## Highlights

- **Tap-to-toggle** with the right `Option` (⌥) key, or switch to hold-to-talk.
- **Three modes**: dictation, ask/command (rewrite a selection or answer a question), and translation.
- **Typeless-style UI**: recording pill with live waveform, Thinking state, translation language bar, ask-anything hint bar, and a large answer panel.
- **Anti-hallucination guards**: silence gate, duration/length plausibility check, LLM balloon check.
- **Local history** (JSONL) and a configurable, OpenAI-compatible ASR + chat endpoint.
- **Universal binary**: Apple Silicon (arm64) and Intel (x86_64).

## Install

1. Download `Rustype-0.1.0-universal.dmg`.
2. Open the image and drag **Rustype.app** into **Applications**.
3. First launch: right-click the app → **Open** → **Open** again (the build is ad-hoc signed and not notarized).
4. Enable Rustype in **System Settings → Privacy & Security → Accessibility**.
5. Launch it: a microphone icon appears in the menu bar.

## Requirements

- macOS 13 or later (tested on macOS 26).
- An OpenAI-compatible endpoint for ASR (`/audio/transcriptions`) and chat (`/chat/completions`) — OpenAI, Groq, SiliconFlow, a local Whisper server, etc.

## SHA256

```
bbcbabfec4e3e54d639e6314510858a891802b6476520affdc46b496238c213e  Rustype-0.1.0-universal.dmg
```

## Known limitations

- Ad-hoc signed: Gatekeeper requires right-click → Open on first launch.
- macOS only.
- Audio is uploaded as WAV (no Opus encoding yet).
- "Ask anything" external actions (open links, create documents) are not implemented.
