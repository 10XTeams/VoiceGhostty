<div align="center">

<img src="Resources/AppIcon.png" width="120" alt="VoiceGhostty icon" />

# VoiceGhostty

**A voice-first terminal for Claude Code — hold space, talk, and your words land at the command line.**

Native macOS. Push-to-talk dictation, natural-language → command, tabs & splits, and one-click Claude Code *skill* loading.

[![Build & Release](https://github.com/10XTeams/VoiceGhostty/actions/workflows/release.yml/badge.svg)](https://github.com/10XTeams/VoiceGhostty/actions/workflows/release.yml)
[![Latest release](https://img.shields.io/github/v/release/10XTeams/VoiceGhostty?sort=semver)](https://github.com/10XTeams/VoiceGhostty/releases)
[![Stars](https://img.shields.io/github/stars/10XTeams/VoiceGhostty?style=social)](https://github.com/10XTeams/VoiceGhostty/stargazers)
![Platform](https://img.shields.io/badge/macOS-13%2B-black?logo=apple)
![Swift](https://img.shields.io/badge/Swift-5%2F6.1-orange?logo=swift)

[**中文文档 →**](README.zh-CN.md)

</div>

---

> [!NOTE]
> **Replace this line with a hero GIF.** Record ~10s: hold <kbd>Space</kbd>, say *"list the biggest files here"*, watch it turn into a command at the cursor, then hit Enter. Save it to `docs/demo.gif` and reference it here. This one image drives most of the stars.

## Why

Working with [Claude Code](https://claude.com/claude-code) means a lot of "type a command, run it, read output, type again." VoiceGhostty keeps your hands off the keyboard for the *talking* parts:

- **Talk instead of type.** Hold <kbd>Space</kbd> to dictate straight into the shell prompt.
- **Say what you want, get the command.** Natural-language mode turns *"undo the last commit but keep the changes"* into `git reset --soft HEAD~1` — with a one-line explanation before you run it.
- **Never surprise-executes.** Recognized text is dropped at the cursor with newlines stripped. **You** press Enter. Always.
- **Load Claude Code skills on the fly.** A 🧩 panel syncs skills from your library into the current project's `.claude/skills/`, then fires `/reload-skills`.

## Features

| | |
|---|---|
| 🎙 **Push-to-talk** | Hold <kbd>Space</kbd> (≥0.25s) to record, release to stop. A quick tap still types a space. |
| 🧠 **Two voice modes** | *Dictation* (local Ollama model cleans filler words & typos, offline) and *Natural language* (Claude API or Apple on-device model → command + explanation). |
| 🌏 **Bilingual recognition** | Chinese & English raced through two recognizers at once — even mixed speech like *"help me ls this folder."* |
| 🗂 **Tabs & splits** | `⌘T` / `⌘W`, `⌘1…⌘9`, split with `⌘⇧D` / `⌘⇧E`, focus-follows-click routing. |
| 🧩 **Dynamic skill loading** | Checkbox panel: pick skills from a library, apply, and they're copied into `cwd/.claude/skills/` — unchecking removes them. Auto-runs `/reload-skills`. |
| 🎨 **Themes & search** | Dark / Light / Solarized Dark / Dracula, `⌘F` scrollback search, live font resize. |
| ✨ **Claude quick-launch** | Save project dirs; one click runs `cd <dir> && claude` in the active pane. |
| 🔒 **Safe by design** | Voice never auto-presses Enter. On-device speech recognition. API keys stay in a local config file. |

## Install

### Option A — download a build (fastest)

Grab the latest `.zip` from [**Releases**](https://github.com/10XTeams/VoiceGhostty/releases), unzip, and because the build is ad-hoc signed (not notarized) clear the quarantine flag once:

```bash
xattr -cr VoiceGhostty.app
open VoiceGhostty.app
```

Grant **Microphone** and **Speech Recognition** when macOS prompts.

### Option B — build from source

```bash
git clone https://github.com/10XTeams/VoiceGhostty.git
cd VoiceGhostty
./make-app.sh          # builds release, packages the .app, launches it
```

> Permission dialogs need the `Info.plist` inside an app bundle, so use `make-app.sh` — don't `swift run` directly.

Requires **macOS 13+**. The natural-language *Apple on-device* backend needs macOS 26+ with Apple Intelligence; without it, set an Anthropic API key to use Claude instead.

## Configuration

`~/.config/voiceghostty/config` — simple `key = value`, `#` for comments:

```ini
font-family = Menlo
font-size   = 14
theme       = dracula          # dark | light | solarized-dark | dracula

# Natural-language mode
llm-provider = auto            # auto | claude | apple
llm-model    = claude-opus-4-8
llm-api-key  = sk-ant-...       # or leave blank to read $ANTHROPIC_API_KEY

# Dictation correction (local, offline)
correction      = on
llm-local-model = qwen3:1.7b
llm-local-url   = http://127.0.0.1:11434

# Skill library the 🧩 panel lists from
skill-library-dir = ~/.claude/skills
```

> Apps launched from Finder don't inherit your shell env, so put the API key in the config file and `chmod 600` it. Full details in [README.zh-CN.md](README.zh-CN.md).

## How it works

```
Mic (AVAudioEngine)
  → on-device recognition (SFSpeechRecognizer)
  → text
     ├─ [Dictation]        local Ollama model cleans it up (offline, silent fallback)
     └─ [Natural language] LLM → command + explanation
                              ├─ Claude API (structured outputs)
                              └─ Apple on-device model (FoundationModels)
  → strip newlines → drop at terminal cursor  ★ never auto-Enter ★
  → you press Enter → SwiftTerm → PTY → zsh
```

Built on [SwiftTerm](https://github.com/migueldeicaza/SwiftTerm) and Apple's Speech framework.

## Roadmap

- [x] SwiftTerm shell, voice dictation, editable confirm-before-run
- [x] Tabs / splits / themes / search / config file
- [x] Natural language → command (Apple on-device **and** Claude API)
- [x] Dictation correction via local model
- [x] Dynamic Claude Code skill loading
- [ ] Command-context awareness, dangerous-command tiering, voice edits, TTS readback
- [ ] Long-term: migrate terminal core to libghostty

## Contributing

Issues and PRs welcome. If VoiceGhostty is useful to you, a ⭐ genuinely helps others find it.
