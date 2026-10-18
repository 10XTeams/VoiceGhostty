# VoiceGhostty — Feature Inventory

A complete inventory of what the app currently does, derived from the source. Each row names the file that
implements it, so this doubles as a map of the codebase.

[中文版 →](FEATURES.zh-CN.md)

**Stack:** Rust. UI in [egui](https://github.com/emilk/egui) / eframe, terminal emulation by
[alacritty_terminal](https://github.com/alacritty/alacritty), audio capture and the chime through
[cpal](https://github.com/RustAudio/cpal), optional in-process [whisper-rs](https://github.com/tazz4843/whisper-rs)
(whisper.cpp), HTTP via reqwest (rustls, no system OpenSSL). Three crates: `vg-core` (config, themes, LLM, shell
integration, loader — no GUI dependency), `vg-voice` (recording, preprocessing, recognition engines) and `vg-app`
(window, terminal widget, pipeline). ~9.9k lines of Rust in 46 files (~8.4k without unit tests). Cross-platform
(macOS / Linux / Windows); no external service is required to run — with the built-in whisper engine compiled in,
even speech recognition is fully offline.

---

## 1. Voice capture

| Feature | Detail | Source |
|---|---|---|
| Push-to-talk | Hold <kbd>Space</kbd> ≥ `voice-hold-ms` (250 ms by default) to record, release to stop. A quick tap below the threshold is re-sent as a normal space, so typing is unaffected; system key-repeat is swallowed while holding. Only active while a terminal pane has keyboard focus; Space with Ctrl / Alt / ⌘ is not intercepted; while the IME is composing (pinyin not yet committed) Space belongs to the IME. Losing focus abandons the key press and stops any recording in progress. | `crates/vg-app/src/ptt.rs`, `crates/vg-app/src/terminal/view.rs` |
| Toggle recording | <kbd>⌘⇧M</kbd>, or click the voice capsule in the status bar. While recording the capsule turns red and shows "listening", a 28-bar level waveform (sampled roughly every 40 ms) and a timer; click again to stop. | `crates/vg-app/src/app.rs`, `crates/vg-app/src/chrome.rs` |
| Recording | cpal opens the system default input device (or the one named by `voice-device`); f32 / i16 / u16 sample formats, multi-channel audio is averaged down to mono. cpal's `Stream` is not `Send` on macOS, so the stream is created and held on a dedicated thread; the main thread only sends "stop" over a channel and takes the clip back. The RMS level is updated continuously for the waveform. | `crates/vg-voice/src/recorder.rs` |
| Preprocessing | Linear-interpolation resampling to 16 kHz (the native rate of the Whisper family); leading and trailing silence trimmed with a 20 ms window at RMS threshold 0.01, keeping 200 ms of margin at each end so the onset is never clipped. All-silent, or less than 0.3 s of speech → recognition is skipped and the status bar says "didn't hear anything". | `crates/vg-voice/src/clip.rs`, `crates/vg-voice/src/lib.rs` |
| Whole-clip recognition | Not streaming: on release the whole clip goes to the engine, so there is **no live transcript**. Recognition, correction and command generation run on a background thread and report back over a channel; the status-bar capsule shows the stage in turn ("recognizing…", "tidying…", "generating command…"). | `crates/vg-app/src/pipeline.rs` |
| Output cleanup | Every engine's output is normalized the same way: lines joined into one (no space inserted between CJK characters), Whisper's bracketed annotations such as `[BLANK_AUDIO]` / `[Music]` removed. | `crates/vg-voice/src/engine.rs` (`clean_transcript`) |
| Punctuation | `voice-punctuation` / "Automatic punctuation" in Settings, on by default. Implemented as a prompt that steers Whisper-family models: for Chinese it asks for Simplified (otherwise Traditional slips in), with punctuation off it gives an unpunctuated example. Worth turning off when you mostly dictate shell commands — full stops are noise there. | `crates/vg-voice/src/engine.rs` (`TranscribeOptions::prompt`) |
| Permissions | Microphone permission is the system's dialog, which needs the `.app` bundle's `NSMicrophoneUsageDescription` (hence `make-app.sh`; a bare binary is not granted access). If no device is found or the stream fails to start, the error asks you to check the input device and microphone permission. | `crates/vg-voice/src/recorder.rs`, `packaging/macos/Info.plist` |

## 2. Recognition engines

The recognizer is **pluggable**, chosen by `voice-engine`. Every engine implements the same interface (`SpeechEngine`):
take a 16 kHz mono, silence-trimmed clip, return text. The engine is constructed before each recording starts, so a
misconfiguration fails immediately instead of after you have spoken; the status-bar capsule names the current engine
(e.g. `whisper · large-v3-turbo`).

| Engine | Detail | Source |
|---|---|---|
| `whisper` (built-in) | whisper.cpp in-process, behind the `whisper` (CPU) or `whisper-metal` (macOS GPU) cargo feature; needs cmake and a C++ compiler at build time. Models are ggml files: `voice-model` names one, otherwise the first `ggml-*.bin` (sorted by name) in `<config dir>/models/`. **Loaded once per process**, cached by model path and reloaded only when the path changes; the inference `WhisperState` (~460 MB of KV cache and Metal compute buffers) is likewise created once and reused, and the lock is held for the whole recognition. **`audio_ctx` shrinks with the recording length**: Whisper pads every input to 30 s and runs the encoder over all of it, which is where nearly all the latency goes; actual frames (50 per second) plus ~5 s of margin, rounded up to a multiple of 64 and clamped to [512, 1500] — about 3× faster for 4 s of speech on an M3, and the 512 floor exists because a smaller context makes the decoder hallucinate and repeat. Flash attention on; greedy decoding, single segment, no cross-segment context; threads = min(cores, 8). The model is **preloaded** in the background at app start so the first utterance does not wait, and **explicitly released** before exit — otherwise ggml's Metal device is torn down first at `exit` and aborts. | `crates/vg-voice/src/whisper.rs`, `crates/vg-voice/src/lib.rs` |
| `command` (external program) | A `voice-command` template where `{wav}` becomes the path of a 16 kHz mono WAV temp file and `{lang}` the recognition language (both quoted for the platform shell), run through `sh -c` (`cmd /C` on Windows); stdout is the result. Example: `whisper-cli -m ~/models/ggml-base.bin -l {lang} -nt -f {wav}`. A non-zero exit reports the first 300 characters of stderr. Anything that can turn a WAV into text plugs in. | `crates/vg-voice/src/engine.rs` |
| `http` (OpenAI-compatible) | `POST voice-api-url` (default `https://api.openai.com/v1/audio/transcriptions`) as multipart: `model`, `response_format=json`, `file`, `language` unless `auto`, and the prompt above. The key comes from `voice-api-key`, else `VOICE_API_KEY`, else `OPENAI_API_KEY`. A URL pointing at this machine (`127.0.0.1` / `localhost` / `[::1]` / `0.0.0.0`) needs no key and **bypasses the system proxy**. 60 s timeout; error text is taken from the body's `error.message` / `error` / `detail`. OpenAI, Groq and the like, as well as local faster-whisper-server, speaches and whisper.cpp server all speak this protocol. | `crates/vg-voice/src/engine.rs` |
| `off` | Voice disabled; Space is just Space. | — |
| `auto` (default) | Picks the first available: built-in whisper (compiled in and a model found) → external command (`voice-command` set) → HTTP (a key exists, or the URL is local). With none of them the capsule reads "no speech engine available · open Settings" and clicking it opens Settings. | `crates/vg-voice/src/lib.rs` (`build_engine`) |

The recognition language is `voice-language` / "Recognition language" in Settings: `zh` / `en` / `auto` (`zh-CN`,
`en-US` are accepted and reduced to the part before the dash). There is **no** bilingual arbitration with a fallback
rerun as in the old app: one recording is recognized once, in whatever language is set.

## 3. Two voice modes

Switched with the segmented control at the left of the status bar, or <kbd>⌘⇧N</kbd> (`crates/vg-app/src/pipeline.rs`, `VoiceMode`).

### Dictation

Recognized text lands at the cursor **verbatim**, with no rewriting. Dictation is mostly command fragments, paths and filenames, where a small model is likelier to damage the text than improve it — so this path stays word-for-word on purpose. The only processing is replacing newlines with spaces before insertion (see § 4, Safety model). The status bar reads "typed: …".

### Natural language

Speak an intent, get a shell command plus a one-sentence explanation (the status bar shows "『what you said』 → explanation").

The transcript goes through a **local correction pass** before reaching the LLM — filler words and homophone typos are cleaned up first, so the LLM receives a clean intent.

| Feature | Detail | Source |
|---|---|---|
| Local small model | Ollama `/api/chat`, default `http://127.0.0.1:11434` and `qwen3:1.7b`, `temperature 0`. Removes filler words and stutters, best-effort homophone fixes. Few-shot prompted — rules alone make a 1.7B model delete content words. | `crates/vg-core/src/ollama.rs` |
| Never blocks | Service down, timeout (15 s), HTTP failure, unparseable reply → silently passes the raw transcript to the LLM. Correction is a nice-to-have and must never lose your words. `correction = off` disables it entirely. | `crates/vg-core/src/ollama.rs`, `crates/vg-app/src/pipeline.rs` |
| Safety valve | Echoed `/no_think` / `/think` tokens are stripped first; if the corrected text shrinks below 50 % of the original (checked only when the original is longer than 6 characters — "um ls" is supposed to shrink a lot) or balloons past 150 %, the model is assumed to have gone off the rails and the original is kept. | `crates/vg-core/src/ollama.rs` (`sanitize`) |
| Warm-up | With correction enabled, app launch fires a background `/api/generate` that loads the model (`keep_alive: 30m`, also sent with every request), moving the ~10 s cold load ahead of your first sentence. Failures are ignored. | `crates/vg-app/src/app.rs`, `crates/vg-core/src/ollama.rs` |
| Proxy bypass | The reqwest client is built with `.no_proxy()` so a system proxy (Clash/Surge…) cannot intercept the loopback request — through a proxy the model's output was observed to ramble until the timeout. | `crates/vg-core/src/ollama.rs` |
| Structured output | Ollama `format` JSON-schema grammar constrains the reply to a single `text` field — which is also what reliably suppresses qwen3's thinking output (the think switch is unreliable under the chatml template). | `crates/vg-core/src/ollama.rs` |

| Backend | Detail | Source |
|---|---|---|
| Claude API | Direct `POST /v1/messages` (no official Rust SDK), default model `claude-opus-5-5`, `max_tokens 1024`, 30 s timeout. Uses **structured outputs** (`output_config.format: json_schema`) so the `{command, explanation}` shape is guaranteed by the API, and `effort: low` to keep voice latency down. Sends `fallbacks: "default"` with the matching beta header, so when the safety classifier refuses, the server reruns the request on another model within the same call — which is why parsing takes the **last** text block. HTTP 401 / 403 / 429 / 5xx and `stop_reason: refusal` are each turned into a readable one-line message. | `crates/vg-core/src/claude.rs` |
| Local Ollama | The cross-platform, offline fallback when there is no Claude key (replacing the old app's Apple on-device model): the same `/api/chat` with a JSON-schema constraint, 60 s timeout, and the reply parsed leniently with `parse_loose` — the outermost `{…}` is extracted, and if no JSON is found the whole reply is treated as the command; an empty `command` is an error, with the `explanation` as the reason. If the service is unreachable the message points at Ollama and the model. | `crates/vg-core/src/ollama.rs`, `crates/vg-core/src/nl2command.rs` |
| Selection | `llm-provider = auto` (Claude when a key exists, else Ollama) / `claude` (an error without a key) / `ollama`. The API key resolves from the config file first, then `$ANTHROPIC_API_KEY` — an app launched from Finder or the Start menu does not see shell environment variables, so the config file is the primary path. | `crates/vg-core/src/nl2command.rs` |
| Context | The system prompt names the operating system (macOS / Linux / Windows), the user's shell (last segment of `$SHELL`; PowerShell on Windows) and the active pane's working directory to improve command accuracy; it asks that dangerous operations be flagged in the explanation, and that `command` be left empty with a reason when the request cannot be done safely. | `crates/vg-core/src/nl2command.rs` (`instructions`) |

## 4. Safety model

- **Never auto-executes.** Every recognized result is *typed* at the terminal cursor (`type_text`: scroll to the bottom, then write the bytes to the pty with no carriage return). You press <kbd>Return</kbd>.
- **Newlines stripped.** `\r` and `\n` are replaced with spaces and the ends trimmed before insertion, so a stray newline inside a sentence cannot trigger execution. Both the dictation and the natural-language path go through this.
- **Empty results are reported**, not silently dropped ("didn't hear anything").
- **Explanation first.** Natural-language mode surfaces "『intent』 → explanation" in the status bar, with dangerous operations flagged by the prompt contract.
- Only two paths in the whole app press Return, and both are explicit clicks: `cd '<dir>' && claude` from the favourite-projects menu, and `/reload-skills` / `/reload-plugins` after the loader applies (several commands are sent 2 s apart, because a second slash command arriving while claude executes the first stays in its input box unsubmitted).

Source: `crates/vg-core/src/nl2command.rs` (`strip_newlines`), `crates/vg-app/src/terminal/session.rs` (`type_text`), `crates/vg-app/src/app.rs` (`pending_commands`)

## 5. Terminal core

| Feature | Detail | Source |
|---|---|---|
| Real PTY shell | alacritty_terminal's `Term` state machine plus its `EventLoop` (pty I/O runs on its own thread; the UI thread only takes a brief lock on the grid while drawing). Runs `$SHELL` (`/bin/sh` if unset); zsh / bash / fish / sh / ksh get `-l` so they start as a **login shell** (reading `.zprofile` for the full PATH); Windows prefers `pwsh.exe`, falling back to `powershell.exe`, with `-NoLogo`. Starts in the user's home directory. Environment: `TERM=xterm-256color`, `COLORTERM=truecolor`, `TERM_PROGRAM=VoiceGhostty`, `VG_STATE_FILE` (see § 9). | `crates/vg-app/src/terminal/session.rs`, `crates/vg-core/src/shell.rs` |
| Rendering | Each frame recomputes columns and rows from the pane's available area and resizes the pty. The grid is drawn by hand: foreground/background, inverse, dim, underline, strikethrough, wide characters (CJK glyphs are narrower than two cells and are centred across both so spacing stays even), the 256-colour cube and greys, 24-bit truecolour; block / beam / underline cursor, outlined only when unfocused; the glyph under the cursor drawn inverted. | `crates/vg-app/src/terminal/view.rs` |
| Font | egui's bundled fonts have neither CJK glyphs nor symbols like ⌘⇧, so a set is assembled from system fonts at startup: the terminal uses `font-family` (searched by file name in the system font directories, skipping bold/italic variants) → SF Mono on macOS → egui's bundled Hack; the UI uses SF Pro (a variable font, from which Medium and Semibold weights are derived) → Ubuntu; then CJK fallbacks (PingFang / Hiragino Sans GB / Microsoft YaHei / Noto CJK / WenQuanYi) and a symbol font. CJK fonts have a much taller line height, which would float Chinese ~0.21 em above the Latin baseline; the baseline difference is measured at startup and corrected through `y_offset_factor`. Live resize <kbd>⌘+</kbd> / <kbd>⌘=</kbd> / <kbd>⌘-</kbd> / <kbd>⌘0</kbd>, clamped to 8–48; the size is global and applies to every pane. Default 13. | `crates/vg-app/src/fonts.rs`, `crates/vg-app/src/app.rs` |
| Themes | Dark / Light / Solarized Dark / Dracula — each a full 16-colour ANSI palette plus foreground/background/cursor. **The entire UI palette** (tab bar, status bar, popups, buttons, text tiers) is derived from the terminal theme, so switching themes recolours the chrome too — never a purple terminal in a grey frame. The swatches in Settings switch instantly but only for this run; the lasting setting is `theme` in the config file, and `foreground` / `background` can override any theme. | `crates/vg-core/src/theme.rs`, `crates/vg-app/src/style.rs`, `crates/vg-app/src/settings_ui.rs` |
| Scrollback | 10 000 lines per pane by default (`scrolling_history`). Settable from the config file (`scrollback`) or Settings (500–200 000); a Settings change applies immediately to panes that are already open (shrinking drops the oldest lines at once). | `crates/vg-app/src/terminal/session.rs`, `crates/vg-core/src/settings.rs` |
| Clear screen | <kbd>⌘K</kbd> sends a form feed (Ctrl-L) so the shell redraws clean. | `crates/vg-app/src/terminal/session.rs` |
| Selection & copy | Drag to select; double-click selects a word, triple-click a line (clicks within 400 ms and 4 px count as a sequence); a click without a drag clears the selection. <kbd>⌘C</kbd> copies on macOS; on Linux / Windows egui turns Ctrl+C into a copy event too, so **copying needs Shift (Ctrl+Shift+C)** and a bare Ctrl+C remains interrupt (`^C`). <kbd>⌘A</kbd> selects the whole history and screen. A program writing the clipboard through OSC 52 reaches the system clipboard as well. | `crates/vg-app/src/terminal/view.rs` |
| Paste | <kbd>⌘V</kbd> (Ctrl+V elsewhere). With bracketed paste mode on, the text is wrapped in `\e[200~…\e[201~` after removing any end marker from the content, so a paste cannot "escape" the brackets and execute; otherwise newlines become `\r`. Scrolls to the bottom first. | `crates/vg-app/src/terminal/view.rs` |
| Keyboard | xterm-style encoding: arrows / Home / End honour application cursor mode (DECCKM) and use `CSI 1;m X` with modifiers; PageUp / Delete / F1–F12; Ctrl+letter → control characters, Ctrl+Space → NUL; on non-macOS Alt+letter is Meta (ESC prefix), while macOS leaves Option combinations to the system to produce characters; ⌘ combinations are never sent to the terminal. Printable characters arrive as text events. | `crates/vg-app/src/terminal/keys.rs` |
| Input method | The IME candidate window follows the terminal cursor; the pinyin being composed is drawn at the cursor with an underline; Space does not trigger push-to-talk while composing. | `crates/vg-app/src/terminal/view.rs` |
| Mouse wheel | Pixel / line / page deltas accumulate into whole-line scrolling of the history; in the alternate screen (less, vim) with no mouse mode, wheel movement becomes Up/Down arrow keys. | `crates/vg-app/src/terminal/view.rs` |
| Process exit | A pane whose shell exits **closes itself** (final history flush, pty hung up); a tab left empty closes too, and when everything is gone the app quits. | `crates/vg-app/src/app.rs` (`pump`), `crates/vg-app/src/workspace.rs` (`retain`) |

## 6. Tabs

| Feature | Detail |
|---|---|
| Create / close | <kbd>⌘T</kbd> new tab; <kbd>⌘W</kbd> closes the active split first, then the tab. The tab's ✕ and a middle-click close it as well. Closing the last tab quits the app (an explicit action, so no confirmation). |
| Switch | <kbd>⌘⇧]</kbd> / <kbd>⌘⇧[</kbd> cycle (wrapping around), <kbd>⌘1</kbd>…<kbd>⌘8</kbd> jump, <kbd>⌘9</kbd> last tab (Ghostty-compatible). |
| Smart titles | The tab name is derived from each pane's **current directory** (home shown as `~`, split panes joined with `-`). The directory does not rely on shell integration: the shell process's cwd is asked of the OS (`proc_pidinfo` on macOS, `/proc/<pid>/cwd` on Linux), refreshed by the monitor thread every 0.5 s; when the lookup fails the previous value is kept so the title never flickers back. Without a directory the terminal title is used; with no panes, `shell`. The window title is "<active pane's terminal title> — VoiceGhostty". |
| Rename | Double-click a tab to set a custom name; <kbd>Esc</kbd> cancels, <kbd>Return</kbd> or blur commits, an empty name restores the derived title. |
| Chrome | Per tab: index number, status dots (only non-grey ones, at most 4; busy pulses, done glows), title (cut to 26 characters), close button (always on the active tab, on hover elsewhere); width clamped to 120–240; hovering lists the panes' terminal titles; a trailing `+` creates a tab. A hairline along the bottom breaks under the active tab so tab and terminal read as one surface. |

Source: `crates/vg-app/src/workspace.rs`, `crates/vg-app/src/chrome.rs` (`tab_bar`), `crates/vg-app/src/app.rs` (`tab_title`)

## 7. Splits

A tab holds a **split tree**: `Node` is either a `Leaf` (one terminal) or a `Branch` (a row or column of children).
Branches nest, so any number of panes in any arrangement is possible — a column of terminals beside a single one, a
2×2 grid, and so on. The tree is the single source of truth; the pane list and every pane's rectangle are derived from
it each frame. Siblings share the space equally; there is no drag-to-resize.

- <kbd>⌘D</kbd> splits the **active pane** right, <kbd>⌘⇧D</kbd> splits it down, and both repeat. Splitting along
  the axis a pane already sits on adds a **sibling**, so three <kbd>⌘D</kbd> in a row give three evenly-sized
  columns rather than a lopsided ½ + ¼ + ¼ nest. Splitting along the other axis replaces that leaf with a
  two-child branch, which is what lets grids form. The new pane takes focus and **starts its shell in the directory
  the original pane is currently in** (read from the OS for that pane's shell, so splitting right after a `cd` lands in the
  new directory; a directory that no longer exists falls back to home). <kbd>⌘T</kbd> still opens a new tab at home.
- <kbd>⌘W</kbd> closes the active pane and moves focus to the visually previous one (to the right-hand one when the
  leftmost was closed); on the tab's last pane it closes the tab. A branch left holding one child collapses into that
  child, so closing panes never leaves empty scaffolding.
- <kbd>⌘⌥←↑↓→</kbd> moves focus by direction (Ghostty `goto_split` semantics), resolved against the tree: walk up
  to the nearest ancestor split along that direction's axis, step to the sibling on that side, then descend to
  its facing edge — so moving left enters the neighbouring subtree from *its* right-hand side. Works across
  nested splits; a direction with no pane on that side leaves focus where it is.
- <kbd>⌘⌃1…8</kbd> / <kbd>⌘⌃←→</kbd> / <kbd>⌘⌃T</kbd> **send the active pane to another tab** — the same number
  as the tab's own <kbd>⌘N</kbd>, one modifier apart. The shell keeps running, the scrollback comes along and the
  status light never flickers: sessions (pty, terminal state, logger) live in a table keyed by id and the split tree
  stores only ids, so the move is a detach from one tree and an insert into another. It lands to the right of the
  target tab's active pane (where that tab's own <kbd>⌘D</kbd> would put it). Moving a tab's last pane out takes the
  (now empty) tab with it, which makes it a tab merge. <kbd>⌘⌃←→</kbd> wrap around; <kbd>⌘⌃T</kbd> moves the active
  pane out into a new tab right after the current one, and does nothing when the tab is not split.
- egui is an immediate-mode UI with no notion of a view owned by two parents at once, so the old app's
  detach → wait for teardown → re-attach handshake is unnecessary here; a move is one tree edit.
- Clicking a pane routes **voice, search and execution** to it — focus and the active tab switch on mouse-down
  (not on release), since focus should arrive as soon as a drag-select begins.
- Inactive panes are dimmed with a translucent overlay rather than outlined. Separators are the 1 pt spacing
  between panes, showing a background layer through the gap.
- When split, the tab bar carries one status dot per lit pane in the panes' visual order (left→right / top→bottom),
  so you can tell at a glance which pane is busy and which has finished.

Source: `crates/vg-app/src/split.rs`, `crates/vg-app/src/workspace.rs`, `crates/vg-app/src/app.rs` (`terminals`)

## 8. Status lights (busy / done)

A three-state dot — grey (idle), yellow (running), green (finished, your turn) — on every tab; when split, one dot
per non-grey pane in visual order, up to 4, so the tab bar says *which* pane is waiting for you rather than only that
one of them is.

- **Precise mode.** Once a command boundary written by shell integration has been seen, it drives the light: command
  started → yellow, command finished → green, and green persists until you attend to it or the next command starts.
  Boundaries **do not travel over OSC 133** (alacritty_terminal discards OSCs it does not know); instead the zsh hooks
  write `C <n>` / `D <n>` into the pane's own state file (see § 9), which the monitor thread reads every 0.5 s. The
  sequence number matters: a quick command that starts and ends between two polls leaves only `D n`, and the changed
  number proves a command happened, so the light still flashes yellow and settles green; `D 0` only says the shell has
  reached its first prompt and the integration works.
- **Interactive TUIs are the exception.** To the shell, `claude` (or vim, or less) is a single command that runs
  until you quit it, so boundaries alone would pin the light yellow for the whole session. Such an app is detected
  by the tty line discipline — a plain command like `sleep 4` runs with `ICANON` **on**, because zsh restores
  canonical mode before exec'ing it, while an app that reads keystrokes itself turns it off (`tcgetattr` on the
  pty primary reflects the secondary's termios). While a command runs in raw mode the light comes from output
  activity instead: streaming = working (yellow), quiet for ≥ 2 s = your turn (green). At the prompt zle is also
  raw, but this is only consulted while a command is running, so that case never arises.
- **Heuristic fallback.** Without shell integration (a shell other than zsh): terminal bell, or "had output then went
  silent for ≥ 2 s", counts as done; recent output counts as busy.
- The single pane you are actively using stays neutral — a terminal never nags you with its own light — but
  only while VoiceGhostty is in front and that tab is not split. Sent to the background, that pane advances like any
  other, since its finish is exactly what you left to wait for; coming back to the front resets it to grey. In a
  split, every pane reports independently; panes in other tabs always advance.
- **Chime on green** (Settings → Terminal → Done chime, on by default, with a ▶ preview next to it). No audio file
  and no system sound: it is synthesized live on a cpal output stream — three partials (E6 / B6 / E7) with exponential
  decay and a 5 ms attack to avoid a click, 0.9 s long, roughly a glass tone, identical on every platform. Throttled
  to at most one ring every 5 s so a task that flickers done→busy→done, or a split finishing two panes at once,
  sounds like one notification.
- **The chime is stricter than the light.** It rings only for a finish that was *announced* — a bell, or a
  command-end marker — not for one inferred from silence inside an interactive app. A TUI pausing between tool calls
  is indistinguishable from a TUI finishing, so ringing on that inference turned the 5 s throttle into a
  metronome for the length of the task. Such a pause still turns the light green: a dot you can ignore costs
  nothing, a sound you have to ignore costs attention.
- The fallback branch does not attempt that distinction, and must not. Raw mode only means "interactive
  app" *while a command is running* — at the prompt zle holds the tty raw as well — and without command boundaries
  there is no running-command state to gate it with. So without shell integration, silence rings, TUI or not.
- The machine advances on a **dedicated monitor thread**, one round every 0.5 s, off the UI thread — the moment the
  chime matters most is exactly when the window is in the background and egui is not repainting. The same round
  refreshes the working directory (which keeps tab titles current), writes the session log every 10th round and reads
  claude's usage every 4th. The pty I/O thread records only "last output time / output sequence / bell seen" in
  atomics, which the monitor reads without locking.

Source: `crates/vg-core/src/status.rs` (state machine and marker parsing — pure logic with unit tests), `crates/vg-app/src/terminal/session.rs` (`PaneShared::tick`), `crates/vg-app/src/monitor.rs`, `crates/vg-app/src/terminal/proc.rs` (`is_raw_mode`), `crates/vg-voice/src/chime.rs`

## 9. zsh shell integration

Installed through a generated **ZDOTDIR wrapper** at `~/.config/voiceghostty/shell-rs/`, whose `.zshenv` /
`.zprofile` / `.zshrc` / `.zlogin` source your real `~/.z*` first and then append the integration. It is regenerated
every time a pane is opened. Running inside `.zshrc` means everything is in place *before the first prompt is drawn* —
no startup flash, and your own prompt is left untouched. (The directory is `shell-rs/` rather than `shell/` so it never
overwrites the one the old Swift app wrote.)

- **Input line colouring** — a `line-pre-redraw` ZLE hook colours the command you are typing. It re-reads
  `~/.config/voiceghostty/input-color` on every redraw using the `read` builtin (no subprocess), so changing
  the colour in Settings applies live in every open tab; when the file is unreadable the default `#5CC8FF` is used.
  Past output and full-screen TUIs like `claude` are untouched.
- **Command boundaries** — `preexec` / `precmd` hooks write `C <n>` / `D <n>` into `$VG_STATE_FILE` with a zsh builtin
  redirect (no subprocess), driving the precise status lights. They still emit **OSC 133** `C` / `D` markers and an
  **OSC 7** working-directory report — VoiceGhostty itself does not use them, but the same `.zshrc` keeps working in
  other terminals.
- **`COLORTERM=truecolor`** is exported.
- State files live at `~/.config/voiceghostty/run/<app pid>-<pane id>.state` and are deleted when the pane closes; at
  startup any left behind by a process that no longer exists (an instance that was killed) are cleaned up.

Only zsh is instrumented; other shells run unmodified (and therefore get neither precise lights nor input colouring,
falling back to the heuristics of § 8).

Source: `crates/vg-core/src/shell.rs` (`prepare_zdotdir`), `crates/vg-core/src/settings.rs` (`write_input_color_file`), `crates/vg-app/src/terminal/session.rs`, `crates/vg-app/src/monitor.rs` (`clean_stale_state_files`)

## 10. Scrollback search

<kbd>⌘F</kbd> floats a search box over the top-right corner of the terminal area. It searches **literal text** (every
regex metacharacter is escaped); an all-lowercase query is case-insensitive, one with capitals is case-sensitive
(alacritty's smart case). It searches as you type, and the first search always starts at the bottom of the screen and
walks **upwards** — what you are looking for is usually what you just saw; <kbd>Return</kbd> continues upwards to older
matches, <kbd>⇧Return</kbd> goes downwards to newer ones, and the ▲ / ▼ buttons in the box do the same. A match is shown
as the **selection** and scrolled into view, so <kbd>⌘C</kbd> copies it directly; no match shows "no results".
<kbd>Esc</kbd> or ✕ closes the box and clears the selection. Search targets the active pane, each pane remembers its own
query, and while the box is open the keyboard belongs to it rather than the terminal.

Source: `crates/vg-app/src/terminal/search.rs`, `crates/vg-app/src/chrome.rs` (`search_box`)

## 11. Session history files

Each pane's transcript is appended to `~/.config/voiceghostty/history/<folder>-<yyyyMMdd-HHmmss>.log`, where
`<folder>` is the directory the pane was working in when the file was created (panes all start in the home
directory, so waiting for real output usually names the file after the project you `cd`'d into; still at home it is
`home`). Toggleable in Settings; on by default; applies to panes opened from then on.

- **The terminal grid is the source of truth, not the raw pty stream.** By the time a line reaches the grid,
  alacritty has applied every escape sequence — so a redrawing TUI (claude's spinner, its re-rendered message
  blocks) lands as the one final line you actually saw instead of dozens of half-painted ones. Wide-character
  placeholder cells are dropped and trailing whitespace trimmed.
- **Only lines that have scrolled off screen are written**, since anything still on screen can be repainted. The
  final write, on pane close or app quit, takes the on-screen remainder too (minus trailing blank lines).
- **Incremental, not a buffer dump** — the file keeps the whole session even after `scrollback` trims the oldest
  lines out of memory.
- **Positioning uses an anchor.** alacritty does not expose "how many lines have scrolled off in total": while the
  history is not yet full, its growth is exactly the number of new lines (precise); once it is full and rotating, or
  when it shrinks (`clear`, a reflow after a width change), the last 8 lines written are looked up in the current
  history as an anchor and writing resumes after them; if the front of the anchor has already rotated out, its tail
  must sit at the very top of the history, which is matched too. If the anchor has been pushed out entirely (a burst
  outran the whole scrollback between flushes, e.g. `cat`ing a huge file), the gap is recorded as
  `[… lines dropped: scrollback limit reached …]` rather than silently lost.
- The alternate screen buffer (vim, less) is skipped: it has no scrollback and its own row numbering, so logging
  it would only interleave garbage.
- Appended every 5 s (every 10th monitor round), plus a final flush when the pane closes or the app quits. If the file
  cannot be opened (permissions, etc.) logging gives up silently instead of complaining on every flush.

Source: `crates/vg-app/src/terminal/history.rs`, `crates/vg-app/src/terminal/session.rs` (`flush_history`)

## 12. Claude context usage

While a pane is running claude, the right end of the status bar shows how full its context window is: `claude`, a
thin bar, `65K / 1M` and `7%`. It disappears the moment claude exits.

The numbers are **not scraped off the screen** — they come from claude's own record. Claude Code appends one
JSON object per record to `~/.claude/projects/<cwd with / turned into ->/<session-id>.jsonl`, and every
assistant record carries that request's `usage`. Context = `input + cache_creation + cache_read + output`
(`cache_read` dominates it, and it is still context).

- **Free and unintrusive.** No extra API call, no cooperation needed from claude, and nothing to break when
  claude changes what its screen looks like — it changes the screen, not this file.
- **One request behind.** The record is written once the response completes, so **a request in flight is not
  counted yet**; the tooltip says so.
- **Recognising claude.** `tcgetpgrp` on the pty primary gives the foreground process group; the executable's **path**
  (`proc_pidpath` on macOS, `/proc/<pid>/exe` on Linux) then says whether it contains "claude". The native installer
  execs `~/.local/share/claude/versions/<version>`, so the process *name* is a bare version number and only the path
  still carries the clue. The working directory is asked of that process too, not of the shell. An npm/node install
  execs "node" and is not recognised. Windows has none of this probing, so the feature is absent there.
- **Both spellings of the project folder are tried.** Some claude versions turn `.` into `-` as well, so both
  `-Users-me-my.app` and `-Users-me-my-app` are looked up and the most recently written `.jsonl` wins.
- **The window comes from the model identity.** A `[1m]` suffix in the transcript's model attachment
  (`claude-opus-5-5[1m]`) means 1M, otherwise 200K. A reading that overflows the assumed window is itself proof
  of a long-context model we do not know — believe the tokens rather than pinning the bar at 100%.
- **Read incrementally, the same way the history file is written.** The first poll parses the whole transcript
  (the model identity lives near the top); after that only the appended bytes are read, a record claude is
  still writing waits for the next poll, and a file shorter than the cursor (rotated or rewritten) is re-read from the
  start. Each line is substring-filtered (`"usage"` / `"modelId"`) before JSON parsing — parsing every record of a
  multi-MB transcript costs more than everything else combined. Subagent turns (`isSidechain`) are excluded — they
  burn their own window.
- **Every 2 s** (every 4th monitor round). Colours: accent below 70%, orange 70–90%, red above 90% (compaction
  happens at roughly 90%).
- Two panes running claude in the same project read the **same file** (the most recently written one): the
  transcript names no pid, so there is nothing to match a session to a pane by. The tooltip shows the token counts,
  model, session id and file, so a reading is at least attributable.

Source: `crates/vg-core/src/claude_usage.rs`, `crates/vg-app/src/terminal/proc.rs` (`claude_working_directory`), `crates/vg-app/src/terminal/session.rs`, `crates/vg-app/src/widgets.rs` (`claude_meter`)

## 13. Claude Code loader (🧩)

Opened with <kbd>⌘L</kbd> or the 🧩 button in the status bar. One centred panel, **three tabs — Skill / Agent / Plugin**.
The target project is the active pane's working directory (when it cannot be determined the panel says so); hovering
the directory name shows where each of the three kinds goes. A folder added in Settings is never asked *what it is*,
only *what it has*, so the same folder can contribute its skills, its agents and its plugin at once, each landing in
the matching tab. Add as many folders as you like; inside a tab they stay one group per folder.

The tabs are a view of one list, not three panels: checkboxes ticked in one tab survive switching to another,
**Apply commits every tab at once** (the button is enabled only when something changed), and each tab's title
carries its item count so an unvisited tab is worth visiting. An empty tab explains which marker the added folders
are missing rather than disappearing.

### 13.1 What a folder is scanned for

Scanning is per kind (`crates/vg-core/src/loader.rs`), reading the same markers Claude Code itself reads:

| Kind | Found by | Looked for in |
|---|---|---|
| Plugin | `.claude-plugin/marketplace.json` (each of its `plugins[]`; a marketplace without a readable name takes the folder's name), else `.claude-plugin/plugin.json` | the folder's root |
| Skill | a subfolder holding `SKILL.md` | `<folder>/skills/` and `<folder>/` — **only when the folder declares no plugin** |
| Agent | a `.md` file opening with YAML front matter (`---`), judged from its first 512 bytes | `<folder>/agents/` and `<folder>/` — **only when the folder declares no plugin** |

- **A folder declaring a plugin contributes only that plugin** — it is not searched for skills or agents at
  all. What is inside it is the plugin's own and arrives when the plugin loads; offering it separately would
  only invite loading the same thing twice, as a detached copy under `.claude/`. The rule goes by the
  manifest, not the plugin count: a marketplace listing nothing loadable is still a plugin folder. A broken manifest
  (invalid JSON) counts as no plugin, and the folder's skills and agents remain loadable.
- Everywhere else, `SKILL.md` and front matter are what keep a folder's build directories and its
  `README.md` out of the list.
- **Legacy fallback**: a folder with no `SKILL.md` anywhere is an old-style library, where every subfolder is
  taken to be a skill; with no front matter anywhere, every top-level `.md` (bar `README.md`, `LICENSE.md`,
  `CLAUDE.md` and similar documents) is an agent — so a library assembled before those conventions still lists.
- Hidden files and folders (leading `.`) are always skipped; listings are sorted by name.

### 13.2 One name, one folder

A skill, an agent and a plugin each end up under a single project-wide name (`.claude/skills/<name>`,
`.claude/agents/<name>.md`, one `enabledPlugins` key), so two folders offering the same name cannot both be
loaded. **The first folder in the Settings list to offer a name owns it**; a later folder offering the same
name gets a disabled row badged *duplicate*, whose tooltip names the folder that already provides it and how to
change that (remove that folder in Settings, or reorder). The panel heads the current tab with a count of what it
skipped, and Settings warns the moment a folder is added how many of its items collide with the folders above — no
silent coin toss, and nothing overwritten.

### 13.3 Applying

- **Skills and agents** are copied into `cwd/.claude/skills/` and `cwd/.claude/agents/` (a skill folder is copied
  recursively); unchecking deletes that one item and touches nothing else. Existing targets are never overwritten,
  and per-item failures are collected into the result without aborting the rest.
- **Plugins** are not copied. Checking one writes two things into the project's `settings.local.json`: the
  switch in `enabledPlugins`, and a `directory`-source registration in `extraKnownMarketplaces` pointing at
  the folder. Unchecking removes the switch, and once no plugin from a marketplace is enabled its registration
  goes too (only when its path really points at our folder), so the file never accumulates paths nothing uses.
  `settings.local.json` rather than `settings.json` because the registration must name the folder by **absolute
  path** — a fact about this machine, meaningless in a file shared through the repo.
- Only `enabledPlugins` and `extraKnownMarketplaces` are rewritten, leaving every other setting untouched,
  and marketplace entries this panel did not register are left alone. The file is written to a temp file and
  renamed into place, so a crash mid-write cannot leave corrupt settings. A file that does not exist with nothing
  to write is not created — merely browsing the panel never litters a project with a stub file. A
  `settings.local.json` hand-edited into invalid JSON is reported, not overwritten.
- **On success, reload commands are typed into the terminal**: `/reload-skills` when skills or agents changed,
  `/reload-plugins` when plugins changed, 2 s apart when both apply (see § 4), so a running Claude session picks the
  changes up at once. The status line reads "synced: copied N, removed M · plugins enabled X, disabled Y, sent …",
  and the list is rescanned so the checkboxes reflect what is actually on disk.
- A **bare plugin folder** (only `plugin.json`) cannot be checked and is badged *no marketplace*: project settings
  reference a plugin as `name@marketplace`, and such a folder declares no marketplace. The tooltip says to add the
  marketplace folder that lists the plugin instead.
- Items already in the project that no added folder offers are still listed (checked), so something left
  behind by a folder since removed stays switchable; unchecking removes it for good.

### 13.4 The list itself

- Items are pre-checked when they are already loaded in the project; the whole row is clickable.
- **One group per folder** inside each tab, in the Settings order, headed by the folder's name (the full path
  when two folders end in the same name). A trailing **"In the project only"** group holds items the project
  has that no added folder offers.
- Each group folds: click its header to collapse/expand, or use the top-right buttons to **expand all** /
  **collapse all** (shown only with more than one group; they act on the tab on screen, leaving the other tabs'
  folding alone). A collapsed header still shows `checked / total`, and the folding is remembered across panel opens
  (`loader-groups-collapsed` in `settings.json`, keyed by `<tab>/<folder>` so the same folder can be folded in
  one tab and open in another).
- A row's only trailing badge is why it cannot be checked (*duplicate*, *no marketplace*) — where it came from
  is the group header's job; the tooltip shows the source path or the reason.
- Rescan button, live status line, and a **warning (not an error)** for any added path that does not exist or a
  settings file that cannot be read — the remaining folders and the loaded items still list.

Source: `crates/vg-core/src/loader.rs` (scanning, ownership, applying — pure logic with unit tests), `crates/vg-app/src/loader_panel.rs` (UI), `crates/vg-app/src/app.rs` (`loader_window`), `crates/vg-app/src/settings_ui.rs` (`library_section`)

## 14. Claude quick-launch (✨ favourite projects)

The ✨ button in the status bar opens a menu. Save frequently used project directories via a native folder picker
(rfd); clicking one runs `cd '<dir>' && claude` followed by Return in the **active pane** (path single-quoted and
escaped, so spaces are safe; PowerShell's `''` escaping on Windows). Entries show the folder name, the full command on
hover and a trash button to remove them individually, and persist in `settings.json` under `claude-dirs`. The menu lists
them by lowercased folder name; the stored array keeps its insertion order, and adding a directory twice is ignored.

Source: `crates/vg-app/src/chrome.rs` (`projects_menu`), `crates/vg-core/src/shell.rs` (`launch_claude_command`, `quote`)

## 15. Settings panel (⚙️)

<kbd>⌘,</kbd> or the gear in the status bar; a centred window whose subtitle says changes apply immediately.

| Group | Setting | Effect |
|---|---|---|
| Voice | Speech engine | `Auto` / `Built-in whisper` / `HTTP API` / `External command` / `Off`, each with a one-line hint (see § 2). |
| Voice | Recognition language | `中文` / `English` / `Auto-detect`; affects speech recognition only (see § 16). |
| Voice | Automatic punctuation | Whether dictation is punctuated; on by default, worth turning off when mostly dictating shell commands. |
| Terminal | Done chime | Ring when a light turns green; on by default, with a ▶ preview. Takes effect in the monitor thread at once. |
| Terminal | Save session logs | Session history on/off, on by default, applies to panes opened from then on; a folder button opens the log directory. |
| Terminal | Default folder | Where <kbd>⌘T</kbd> new tabs and the first pane at launch start; home by default. The folder button picks a directory, ✕ resets to home; a directory that no longer exists falls back to home. Splits ignore it and follow the original pane (see §7). |
| Terminal | Scrollback lines | A drag-value field for 500–200 000, clamped automatically; applies immediately to panes that are already open. |
| Appearance | Theme | Four swatches (terminal background, three ANSI colour dots and a foreground stroke); clicking recolours the whole UI **for this run only** — the lasting setting lives in the config file. |
| Appearance | Command input colour | Colour picker; mirrored to `~/.config/voiceghostty/input-color` and picked up live by the zsh hook in every open tab. |
| Load folders | Folder list | The source directories the 🧩 panel reads. **Add folder…** takes several at once; hovering a row reveals "move up (higher priority)" and "remove"; the index is the priority for duplicate names; adding a folder immediately reports how many of its items collide with the folders above. Removing every row is allowed and sticks — with no folders, the 🧩 panel lists only what is already loaded in the project. |

A footer line notes that API URLs, the external recognition command, models and the rest live in
`~/.config/voiceghostty/config`; clicking the path reveals its folder in the file manager. The bottom-right corner shows
the version and commit (`VoiceGhostty v0.1.0 (abc1234+)`, see § 18), with the build time on hover.

Settings made here are stored in `~/.config/voiceghostty/settings.json` (keys: `default-language`, `voice-engine`,
`adds-punctuation`, `scrollback`, `save-history`, `input-color`, `done-sound`, `skill-library-dirs`,
`loader-groups-collapsed`, `claude-dirs`, `default-dir`). For the keys it shares with the config file the GUI setting wins; a key never
set in the GUI (`null`) falls back to the config file, which remains the power-user / fallback path. Saving settings also
rewrites the `input-color` file.

Every popup (Settings, 🧩 loader, shortcut reference, quit confirmation) is a centred egui window with no default title
bar — a hand-drawn left-aligned title with a ✕ in the corner, closed by <kbd>Esc</kbd>.

Source: `crates/vg-app/src/settings_ui.rs`, `crates/vg-core/src/settings.rs`, `crates/vg-app/src/kit.rs` (window, setting row, toggle and other widgets), `crates/vg-app/src/style.rs` (design tokens)

## 16. UI language

The UI is **Chinese only**: every user-facing string is written inline at its call site, there is no string table and no
language switch. The "Recognition language" setting (`中文` / `English` / `Auto-detect`) only decides which language
speech recognition listens for (`voice-language`); it does not affect the UI. Low-level error messages coming from
`vg-core` / `vg-voice` (such as `Invalid API Key`, `No speech engine configured`) are English and appear in the status
bar as they are; the LLM prompts are English but ask for the explanation in the language the user spoke.

Source: `crates/vg-app/src/chrome.rs`, `crates/vg-app/src/settings_ui.rs`, `crates/vg-app/src/widgets.rs` (`language_label`)

## 17. Configuration file

`~/.config/voiceghostty/config` (`%APPDATA%\voiceghostty\config` on Windows), minimal `key = value`, `#` comments;
unknown keys, malformed lines and empty values are ignored (never an error). Booleans: `off` / `false` / `no` / `0` are
false, anything else is true. **23 keys:**

| Key | Default | Purpose |
|---|---|---|
| `font-family` | platform monospace | Terminal font family (looked up by file name in the system font directories) |
| `font-size` | `13` | Also the <kbd>⌘0</kbd> reset target; must be > 0 |
| `theme` | `dark` | `dark` / `light` / `solarized-dark` / `dracula`; unknown values fall back to `dark` |
| `foreground` | — | `#rrggbb` override on top of the theme |
| `background` | — | `#rrggbb` override on top of the theme |
| `scrollback` | `10000` | Lines kept per pane; Settings overrides this |
| `llm-provider` | `auto` | `auto` / `claude` / `ollama` (the old spellings `local` and `apple` also map to `ollama`) |
| `llm-model` | `claude-opus-5-5` | Claude model ID |
| `llm-api-key` | — | Falls back to `$ANTHROPIC_API_KEY` |
| `correction` | `on` | `off` disables the natural-language correction pass (dictation is always verbatim) |
| `llm-local-model` | `qwen3:1.7b` | Ollama model, for correction and for command generation without a Claude key |
| `llm-local-url` | `http://127.0.0.1:11434` | Ollama endpoint |
| `skill-library-dir` | `~/.claude/skills` | Loader sources (tilde expanded). Several folders: comma-separate them, or repeat the key (the first occurrence replaces the default, later ones append) — the order is the 🧩 panel's group order and the priority for duplicate names; the Settings list overrides this |
| `voice-engine` | `auto` | `auto` / `whisper` / `http` / `command` / `off` (aliases: `local` = whisper, `api` / `openai` = http, `cmd` / `exec` = command, `none` / `disabled` = off); Settings overrides this |
| `voice-language` | `zh` | `zh` / `en` / `auto`; spellings like `zh-CN` keep the part before the dash; Settings overrides this |
| `voice-punctuation` | `on` | Whether to ask for punctuation; Settings overrides this |
| `voice-api-url` | `https://api.openai.com/v1/audio/transcriptions` | HTTP engine endpoint; a local address needs no key and bypasses the proxy |
| `voice-api-key` | — | Falls back to `$VOICE_API_KEY`, then `$OPENAI_API_KEY` |
| `voice-api-model` | `whisper-1` | Model name for the HTTP engine |
| `voice-command` | — | External command template with `{wav}` and `{lang}` placeholders |
| `voice-model` | first `ggml-*.bin` in `<config dir>/models/` | ggml model path for the built-in whisper engine (tilde expanded) |
| `voice-device` | system default | Microphone device name |
| `voice-hold-ms` | `250` | How long Space must be held to count as push-to-talk |

Source: `crates/vg-core/src/config.rs`, `crates/vg-core/src/paths.rs`

## 18. App & window behaviour

- An eframe native window, 960×600 at launch, minimum 400×240; the window title follows the active pane's terminal
  title ("<title> — VoiceGhostty").
- Layout: tab bar on top (38 pt), status bar at the bottom (34 pt), terminals in between. The status bar is **always in
  the layout**, empty or not, and never wraps past one line (long text truncates, the tooltip holds all of it). A strip
  that appeared and disappeared with each message — or grew a second line — would resize the terminal under whatever is
  running in it, and a SIGWINCH landing mid-frame leaves a full-screen TUI drawn a row off until it repaints itself.
  Result messages fade out after 15 s; errors stay until the next message.
- Status bar, left to right: the voice-mode segmented control, the voice capsule (§ 1), the result of the last action;
  at the right end ✨ favourite projects, 🧩 loader, ⌨ shortcut reference, ⚙️ Settings, and the claude usage meter (§ 12).
- **Closing the window asks first**: the system's close request is intercepted (on macOS the Quit menu item and <kbd>⌘Q</kbd> go the same way: at startup that item's action is changed from a direct `terminate:` to `performClose:`, the same path as the ✕ button, see `crates/vg-app/src/macos.rs`) and a modal
  "Quit VoiceGhostty?" states how many panes' shells would be hung up; <kbd>Return</kbd> quits, <kbd>Esc</kbd> cancels.
  Exceptions: closing the last split or tab with <kbd>⌘W</kbd> / ✕ is an explicit action and quits without asking, as
  does every shell exiting on its own.
- On exit: each pane writes its final log and sends Shutdown to its pty; then the built-in whisper model is released
  (without this, ggml's Metal device is torn down first at `exit` and aborts).
- Startup order: clean stale state files → load `settings.json` on top of the config file → install fonts → apply the
  theme → preload the whisper model in the background → (with correction on) warm up Ollama in the background → open
  the first tab. `alacritty_terminal::tty::setup_env()` lets the shell inherit a correct locale.
- Logging goes through `env_logger`, `RUST_LOG` defaulting to `warn`; whisper.cpp / ggml logs are routed into the same
  sink instead of flooding stderr.
- **Cross-platform**: two shortcut sets, macOS and Linux/Windows (§ 19); Windows runs PowerShell and has no pty probing
  (`tcgetattr` / `tcgetpgrp`) or process-table lookups, so TUI detection, cwd-by-process and the claude meter are absent
  there and tab titles fall back to the terminal title.
- **Packaging** (`make-app.sh`): `cargo build --release -p vg-app --features whisper-metal` (`VG_FEATURES` overrides
  the feature list; empty skips whisper and the cmake requirement) → kill any running instance (otherwise `open` would
  only bring the old process to the front) → assemble `build/VoiceGhostty.app` from `packaging/macos/Info.plist` and
  `AppIcon.icns` → write the version info → ad-hoc codesign → launch; `--no-open` only builds and packages (for CI). The
  microphone permission dialog depends on the bundle's `NSMicrophoneUsageDescription`, so a bare `cargo run` binary is
  not granted access. `LSMinimumSystemVersion` is 13.0.
- **The version number has a single source**: the workspace `version` in `Cargo.toml`. `vg-app`'s `build.rs` injects
  the git commit as the `VG_GIT_COMMIT` environment variable (the packaging script and CI set it themselves, with a
  trailing `+` for uncommitted changes; unset, build.rs asks git) together with the build time `VG_BUILD_EPOCH`; the
  bottom of the Settings window shows `v0.1.0 (abc1234+)`, with version / commit / build time on hover. The packaging
  script also writes the version into `CFBundleShortVersionString`, a build timestamp into `CFBundleVersion` and the
  commit into a custom `VGGitCommit` key. The marketing version does not change between builds, so it alone cannot tell
  old from new; the commit says whether the `.app` you are running contains a given fix.
- Screenshot tests (`shots.rs`, skipped by default) render the real app with a real shell off-screen through wgpu into
  PNGs for visual inspection.

Source: `crates/vg-app/src/main.rs`, `crates/vg-app/src/app.rs`, `crates/vg-app/src/chrome.rs` (`status_bar`, `quit_dialog`), `crates/vg-app/build.rs`, `crates/vg-app/src/version.rs`, `make-app.sh`, `packaging/macos/Info.plist`

## 19. Keyboard shortcuts

The shortcut table is defined once (`crates/vg-app/src/shortcuts.rs`); both key handling and the <kbd>⌘/</kbd> help
window read it, so they cannot disagree. macOS uses ⌘ combinations; on Linux / Windows bare Ctrl combinations are left to
the terminal (Ctrl+T / W / D all mean something in a shell), so most shortcuts use Ctrl+Shift, and split moves add Alt.

| Action | macOS | Linux / Windows |
|---|---|---|
| Talk (release to stop; a quick tap types a space) | Hold <kbd>Space</kbd> | Hold <kbd>Space</kbd> |
| Start / stop recording | <kbd>⌘⇧M</kbd> | <kbd>Ctrl+Shift+M</kbd> |
| Toggle dictation / natural language | <kbd>⌘⇧N</kbd> | <kbd>Ctrl+Shift+N</kbd> |
| New tab | <kbd>⌘T</kbd> | <kbd>Ctrl+Shift+T</kbd> |
| Close split, else close tab | <kbd>⌘W</kbd> | <kbd>Ctrl+Shift+W</kbd> |
| Next / previous tab | <kbd>⌘⇧]</kbd> / <kbd>⌘⇧[</kbd> | <kbd>Ctrl+PageDown</kbd> / <kbd>Ctrl+PageUp</kbd> |
| Jump to Nth / last tab | <kbd>⌘1</kbd>…<kbd>⌘8</kbd> / <kbd>⌘9</kbd> | <kbd>Alt+1</kbd>…<kbd>Alt+8</kbd> / <kbd>Alt+9</kbd> |
| Split right / split down | <kbd>⌘D</kbd> / <kbd>⌘⇧D</kbd> | <kbd>Ctrl+Shift+D</kbd> / <kbd>Ctrl+Shift+E</kbd> |
| Move split focus by direction | <kbd>⌘⌥←↑↓→</kbd> | <kbd>Ctrl+Alt+←↑↓→</kbd> |
| Send this split to the Nth tab | <kbd>⌘⌃1</kbd>…<kbd>⌘⌃8</kbd> | <kbd>Ctrl+Alt+1</kbd>…<kbd>Ctrl+Alt+8</kbd> |
| Send this split to the previous / next tab | <kbd>⌘⌃←</kbd> / <kbd>⌘⌃→</kbd> | <kbd>Ctrl+Alt+Shift+←</kbd> / <kbd>Ctrl+Alt+Shift+→</kbd> |
| Send this split out to a tab of its own | <kbd>⌘⌃T</kbd> | <kbd>Ctrl+Alt+Shift+T</kbd> |
| Font larger / smaller / reset | <kbd>⌘+</kbd> <kbd>⌘=</kbd> / <kbd>⌘-</kbd> / <kbd>⌘0</kbd> | <kbd>Ctrl+Shift++</kbd> <kbd>Ctrl+Shift+=</kbd> / <kbd>Ctrl+Shift+-</kbd> / <kbd>Ctrl+Shift+0</kbd> |
| Clear screen | <kbd>⌘K</kbd> | <kbd>Ctrl+Shift+K</kbd> |
| Search scrollback | <kbd>⌘F</kbd> | <kbd>Ctrl+Shift+F</kbd> |
| Select all | <kbd>⌘A</kbd> | <kbd>Ctrl+Shift+A</kbd> |
| Copy selection / paste | <kbd>⌘C</kbd> / <kbd>⌘V</kbd> | <kbd>Ctrl+Shift+C</kbd> / <kbd>Ctrl+V</kbd> (a bare <kbd>Ctrl+C</kbd> is still interrupt) |
| 🧩 Loader | <kbd>⌘L</kbd> | <kbd>Ctrl+Shift+L</kbd> |
| Settings | <kbd>⌘,</kbd> | <kbd>Ctrl+Shift+,</kbd> |
| Shortcut reference | <kbd>⌘/</kbd> | <kbd>Ctrl+Shift+/</kbd> |

Mouse: double-click a tab to rename it, middle-click to close it, click a pane to focus it, double- / triple-click in the
terminal to select a word / line.
