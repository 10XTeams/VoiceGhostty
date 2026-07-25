# VoiceGhostty — Feature Inventory

A complete inventory of what the app currently does, derived from the source. Each row names the file that
implements it, so this doubles as a map of the codebase.

[中文版 →](FEATURES.zh-CN.md)

**Stack:** SwiftUI + [SwiftTerm](https://github.com/migueldeicaza/SwiftTerm) + Apple `Speech` / `AVFoundation`,
optional `FoundationModels` (macOS 26+). ~2.9k lines of Swift, 19 files, no external services required to run.

---

## 1. Voice capture

| Feature | Detail | Source |
|---|---|---|
| Push-to-talk | Hold <kbd>Space</kbd> ≥ 0.25 s to record, release to stop. A quick tap below the threshold is re-sent as a normal space, so typing is unaffected. System key-repeat is swallowed while holding. | `Voice/PushToTalk.swift` |
| Toggle recording | <kbd>⌘⇧M</kbd> or the mic button in the toolbar; the button turns red while recording. | `ContentView.swift` |
| Live transcript | Partial results stream into the toolbar next to the mic while you speak (head-truncated to one line). | `Voice/SpeechRecognizer.swift` |
| Voice-activity gate | Recognition starts only once RMS volume crosses a threshold; a 6-buffer pre-roll is replayed into the recognizer so the first syllable is never clipped. This avoids leading silence tripping recognizer error 1110. | `Voice/SpeechRecognizer.swift` |
| Permissions | Speech-recognition and microphone authorization are requested on first use, with a plain-language error naming the exact System Settings pane when denied. A generation counter cancels a recording the user aborted mid-prompt. | `Voice/SpeechRecognizer.swift` |
| On-device recognition | `SFSpeechRecognizer` runs locally — audio is not uploaded. | `Voice/SpeechRecognizer.swift` |

## 2. Bilingual recognition (zh-CN / en-US)

Exactly **one** recognizer runs at a time — two concurrent on-device recognizers preempt each other and
randomly leave only one producing results.

- The **primary** locale comes from Settings (`zh-CN` by default) and drives live recognition while you speak.
- All audio is also captured into a buffer.
- On stop, the primary result is **arbitrated**: Chinese-primary is trusted only if the transcript contains
  Han characters (the zh recognizer otherwise transliterates English into wrong Han); English-primary is
  trusted if non-empty.
- If arbitration fails, the **fallback** locale is run **once** over the full captured audio — by then the
  primary task has ended, so there is no preemption.
- A 2 s watchdog on each stage guarantees a result is always delivered.

Source: `Voice/SpeechRecognizer.swift`

## 3. Two voice modes

Switched with the segmented picker in the toolbar (`Voice/VoiceMode.swift`).

### Dictation (`text.cursor`)

Recognized text goes through an optional **local correction pass** before landing at the cursor.

| Feature | Detail | Source |
|---|---|---|
| Local small model | Ollama at `http://127.0.0.1:11434`, default `qwen3:1.7b`. Removes filler words and stutters, best-effort homophone fixes. Few-shot prompted — rules alone make a 1.7B model delete content words. | `Command/OllamaClient.swift` |
| Never blocks | Service down, timeout (15 s), unparseable reply → silently uses the raw transcript. Correction is a nice-to-have and must never lose your words. | `Command/OllamaClient.swift` |
| Safety valve | If the corrected text shrinks below 50 % or balloons past 150 % of the original, the model is assumed to have gone off the rails and the original is kept. | `Command/OllamaClient.swift` |
| Warm-up | On app launch a background `/api/generate` loads the model (`keep_alive: 30m`), moving the ~10 s cold load ahead of your first sentence. Failures are ignored. | `ContentView.swift`, `Command/OllamaClient.swift` |
| Proxy bypass | The URLSession is configured with an empty `connectionProxyDictionary` so a system proxy (Clash/Surge…) cannot intercept the loopback request. | `Command/OllamaClient.swift` |
| Structured output | Ollama `format` JSON-schema grammar constrains the reply to a single `text` field — which is also what reliably suppresses qwen3's thinking output. | `Command/OllamaClient.swift` |

### Natural language (`sparkles`)

Speak an intent, get a shell command plus a one-sentence explanation shown in the status bar.

| Backend | Detail | Source |
|---|---|---|
| Claude API | Direct `POST /v1/messages` (no Swift SDK). Uses **structured outputs** (`output_config.format: json_schema`) so the `{command, explanation}` shape is guaranteed by the API, and `effort: low` to keep voice latency down. Handles HTTP errors and `stop_reason: refusal` with a readable one-line message. | `Command/ClaudeClient.swift` |
| Apple on-device | `FoundationModels` `SystemLanguageModel` (macOS 26 + Apple Intelligence). Availability is checked up front and each unavailable reason maps to a specific hint. Compiled behind `#if canImport`, and asks for plain JSON rather than `@Generable` so the project still builds on a CLT-only toolchain. | `Command/NL2Command.swift` |
| Selection | `llm-provider = auto` (Claude when a key exists, else on-device) / `claude` / `apple`. API key resolves from the config file first, then `$ANTHROPIC_API_KEY`. | `Command/NL2Command.swift` |
| Context | The active pane's current working directory is injected into the system prompt to improve command accuracy. | `Command/NL2Command.swift` |
| Tolerant parsing | The on-device path extracts the outermost `{…}` from the reply; if no JSON is found, the whole reply is treated as the command. | `Command/NL2Command.swift` |

## 4. Safety model

- **Never auto-executes.** Every recognized result is *typed* at the terminal cursor. You press <kbd>Return</kbd>.
- **Newlines stripped.** `\r` and `\n` are replaced with spaces before insertion, so a stray newline inside a
  sentence cannot trigger execution.
- **Empty results are reported**, not silently dropped ("Didn't catch that, please say it again").
- **Explanation first.** Natural-language mode surfaces a one-line explanation in the status bar, with
  dangerous operations flagged by the prompt contract.
- The only paths that press Return are explicit clicks: Claude quick-launch and the skill panel's
  `/reload-skills`.

Source: `ContentView.swift` (`insertIntoTerminal`)

## 5. Terminal core

| Feature | Detail | Source |
|---|---|---|
| Real PTY shell | SwiftTerm `LocalProcessTerminalView` runs `$SHELL` as a **login shell** (`-zsh`), starting in the user's home directory (an `.app` would otherwise start at `/`). | `Terminal/TerminalController.swift` |
| Font | System monospace (SF Mono) by default, or any named family from the config. Live resize <kbd>⌘+</kbd> / <kbd>⌘=</kbd> / <kbd>⌘-</kbd> / <kbd>⌘0</kbd>, clamped to 8–48 pt and applied to every open session. | `Terminal/SessionStore.swift`, `Config/Config.swift` |
| Themes | Dark / Light / Solarized Dark / Dracula — each a full 16-color ANSI palette plus foreground/background/cursor. Switchable live from **View → Theme**; the config file can override fg/bg on top of any theme. | `Config/Theme.swift` |
| Clear screen | <kbd>⌘K</kbd> sends a form feed so the shell redraws clean. | `Terminal/SessionStore.swift` |
| Copy / paste / select-all | <kbd>⌘C</kbd> / <kbd>⌘V</kbd> / <kbd>⌘A</kbd> via the system Edit menu (SwiftTerm implements the responder-chain methods). | — |
| Process exit | A "Process exited" badge appears on the pane; the session is deliberately **not** auto-closed. | `Terminal/TerminalView.swift` |

## 6. Tabs

| Feature | Detail |
|---|---|
| Create / close | <kbd>⌘T</kbd> new tab; <kbd>⌘W</kbd> closes the active split first, then the tab. At least one tab always survives. |
| Switch | <kbd>⌘⇧]</kbd> / <kbd>⌘⇧[</kbd> cycle, <kbd>⌘1</kbd>…<kbd>⌘8</kbd> jump, <kbd>⌘9</kbd> last tab (Ghostty-compatible). |
| Smart titles | The tab name is derived from each pane's **current directory** (home shown as `~`, split panes joined with `-`), polled live — not from the OSC process title, which some shells set to the computer name. |
| Rename | Double-click a tab (or use its context menu) to set a custom name; <kbd>Esc</kbd> cancels, blur commits, an empty name restores the derived title. |
| Chrome | Horizontally scrollable bar, per-tab index number, status dot, split indicator, enlarged close hit-area, `+` button. |

Source: `Terminal/SessionStore.swift`, `Terminal/TerminalView.swift`, `VoiceGhosttyApp.swift`

## 7. Splits

- <kbd>⌘D</kbd> split right, <kbd>⌘⇧D</kbd> split down — up to 2 panes per tab.
- <kbd>⌘⌥←↑↓→</kbd> moves focus by direction (Ghostty `goto_split` semantics); a direction perpendicular to
  the split axis is ignored.
- Clicking a pane routes **voice, search and execution** to it — focus is tracked through `mouseDown` and
  synced back into the session store.
- The inactive pane is dimmed with a translucent overlay rather than outlined.
- Each pane carries its own status dot when split, so you can tell which side is busy and which finished.

Source: `Terminal/SessionStore.swift`, `Terminal/TerminalView.swift`

## 8. Status lights (busy / done)

A three-state dot — gray (idle), yellow (running), green (finished, your turn) — on every tab, and on every
pane when split. The tab dot shows the most important state among its panes.

- **Precise mode.** Once any OSC 133 marker is seen, command boundaries come from shell integration:
  `C` = command started → yellow, `D` = command finished → green, and green persists until you attend to it.
- **Heuristic fallback.** Without shell integration: terminal bell, or "had output then went silent for ≥ 2 s",
  counts as done; recent output counts as busy.
- The single pane you are actively using stays neutral — a terminal never nags you with its own light. In a
  split, both panes report independently.
- Polled every 0.5 s alongside a working-directory refresh, which is what keeps tab titles current.

Source: `Terminal/SessionStore.swift`, `Terminal/TerminalController.swift`

## 9. zsh shell integration

Installed through a generated **ZDOTDIR wrapper** at `~/.config/voiceghostty/shell/`, whose dotfiles source
your real `~/.zshenv` / `~/.zprofile` / `~/.zshrc` / `~/.zlogin` first and then append the integration. Running
inside `.zshrc` means everything is in place *before the first prompt is drawn* — no startup flash, and your own
prompt is left untouched.

- **Input line coloring** — a `line-pre-redraw` ZLE hook colors the command you are typing. It re-reads
  `~/.config/voiceghostty/input-color` on every redraw using the `read` builtin (no subprocess), so changing
  the color in Settings applies live in every open tab. Past output and full-screen TUIs like `claude` are
  untouched.
- **OSC 133 markers** — `preexec` / `precmd` hooks emit command start/end, driving the precise status lights.
- **`COLORTERM=truecolor`** is exported.

Only zsh is instrumented; other shells run unmodified.

Source: `Terminal/TerminalController.swift` (`prepareZDotDir`), `Config/AppSettings.swift`

## 10. Scrollback search

<kbd>⌘F</kbd> opens a search bar under the terminal: <kbd>Return</kbd> next, <kbd>⇧Return</kbd> previous,
<kbd>↑</kbd> previous, live `current/total` match count (or "No matches"), <kbd>Esc</kbd> / "Done" closes and
clears highlighting. Search targets the active pane.

Source: `ContentView.swift`

## 11. Claude Code skill loader (🧩)

A checkbox synchronizer between a **skill library** and the active pane's `cwd/.claude/skills/`.

- Lists every non-hidden subfolder of the library as a loadable skill; skills already present in the project
  are pre-checked.
- Two groups: already loaded in the project, and available in the library — plus a **"Local only"** marker for
  skills that exist in the project but not the library (unchecking one deletes it and it cannot be re-added
  from here).
- **Apply** copies newly checked skills in and deletes unchecked ones, touching nothing else in the directory.
  Existing targets are never overwritten. Per-skill failures are reported without aborting the rest.
- On a successful change it types `/reload-skills` into the terminal so a running Claude session picks the
  skills up immediately.
- Rescan button, live status line, and a warning (not an error) when the library path does not exist.

Source: `Skills/SkillPanelView.swift`

## 12. Claude quick-launch (✨)

Save frequently used project directories via a native folder picker; clicking one runs
`cd '<dir>' && claude` in the **active pane** (path single-quoted and escaped, so spaces are safe). Entries
show the folder name with the full path on hover, can be removed individually, and persist in `UserDefaults`.

Source: `ContentView.swift`

## 13. Settings panel (⚙️)

One gear button, two tabs — editable settings and the full shortcut reference.

| Setting | Effect |
|---|---|
| Default language | `中文` / `English`. Switches the **speech-recognition primary locale and the entire UI language** together. |
| Command input color | Color picker; mirrored to `~/.config/voiceghostty/input-color` and picked up live by the zsh hook in every open tab. |
| Default skill library folder | The source directory the 🧩 panel lists skills from. |

Settings written here (`UserDefaults`) take precedence over the config file for the keys they share; the config
file remains the power-user / fallback path.

All three toolbar panels (settings, skills, Claude menu) are drawn as **in-window overlays** anchored top-right,
not native popovers — so they can never spill past the window edge, and clicking outside dismisses them.

Source: `SettingsView.swift`, `ShortcutsHelpView.swift`, `Config/AppSettings.swift`, `ContentView.swift`

## 14. Bilingual UI

Every user-facing string carries both languages inline — `loc("Done", "完成")` — so there is no string table to
keep in sync. Views observe a shared `Loc` object, so switching the language in Settings re-renders the whole UI
immediately, in lockstep with the recognition language.

Source: `Config/Loc.swift`

## 15. Configuration file

`~/.config/voiceghostty/config`, minimal `key = value`, `#` comments, unknown keys and malformed lines ignored
(never an error). **12 keys:**

| Key | Default | Purpose |
|---|---|---|
| `font-family` | system mono | Terminal font family |
| `font-size` | `12` | Also the <kbd>⌘0</kbd> reset target |
| `theme` | `dark` | `dark` / `light` / `solarized-dark` / `dracula` |
| `foreground` | — | `#rrggbb` override on top of the theme |
| `background` | — | `#rrggbb` override on top of the theme |
| `llm-provider` | `auto` | `auto` / `claude` / `apple` |
| `llm-model` | `claude-opus-4-8` | Claude model ID |
| `llm-api-key` | — | Falls back to `$ANTHROPIC_API_KEY` |
| `correction` | `on` | `off` disables dictation correction |
| `llm-local-model` | `qwen3:1.7b` | Ollama correction model |
| `llm-local-url` | `http://127.0.0.1:11434` | Ollama endpoint |
| `skill-library-dir` | `~/.claude/skills` | Skill library source (tilde expanded) |

Source: `Config/Config.swift`

## 16. App & window behavior

- Forces `.regular` activation policy so an SPM executable behaves like a real foreground app.
- Opens at 80 % of the screen's visible frame (excluding menu bar and Dock), centered; minimum 800×500.
- Quits when the last window closes.
- <kbd>⌘W</kbd> deliberately replaces the system **File → Close**, which would otherwise steal the shortcut and
  close the whole window instead of one tab.
- `make-app.sh` builds release, kills any running instance, packages the `.app` with `Info.plist` + icon,
  ad-hoc signs it and launches it. Permission dialogs require the bundle, so `swift run` is not enough.

Source: `VoiceGhosttyApp.swift`, `make-app.sh`, `Resources/Info.plist`

## 17. Keyboard shortcuts

| Keys | Action |
|---|---|
| Hold <kbd>Space</kbd> | Talk (release to stop; a quick tap types a space) |
| <kbd>⌘⇧M</kbd> | Start / stop recording |
| <kbd>⌘T</kbd> | New tab |
| <kbd>⌘W</kbd> | Close split, else close tab |
| <kbd>⌘⇧]</kbd> / <kbd>⌘⇧[</kbd> | Next / previous tab |
| <kbd>⌘1</kbd>…<kbd>⌘8</kbd> / <kbd>⌘9</kbd> | Jump to Nth tab / last tab |
| <kbd>⌘D</kbd> / <kbd>⌘⇧D</kbd> | Split right / split down |
| <kbd>⌘⌥←↑↓→</kbd> | Move split focus by direction |
| <kbd>⌘+</kbd> <kbd>⌘=</kbd> / <kbd>⌘-</kbd> / <kbd>⌘0</kbd> | Font larger / smaller / reset |
| <kbd>⌘K</kbd> | Clear screen |
| <kbd>⌘F</kbd> | Search scrollback |
| <kbd>⌘C</kbd> / <kbd>⌘V</kbd> / <kbd>⌘A</kbd> | Copy / paste / select all |
