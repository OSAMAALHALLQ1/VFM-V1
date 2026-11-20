# Testing

## Phase 7 — advanced features

### Automated

```powershell
powershell -NoProfile -File scripts/cargo.ps1 fmt --all
powershell -NoProfile -File scripts/cargo.ps1 clippy --workspace --all-targets -- -D warnings
powershell -NoProfile -File scripts/cargo.ps1 test --workspace
powershell -NoProfile -File scripts/build-local.ps1 -Action Build
```

Latest validation: **75 tests passing** (64 core including metadata, waveform, OpenSubtitles hash/URL and bookmark rename, 5 wrapper, 6 i18n), Clippy clean, release build succeeds.

- [x] Bookmarks: Ctrl+B adds one, persisted in SQLite; sidebar list with delete/seek and seek-bar markers.
- [x] Chapter count/current from libVLC with a next-chapter control.
- [x] Waveform peaks decoded on a worker with `symphonia`, cached and toggleable from the context menu; video-only clips yield a flat waveform.
- [x] Metadata panel via `lofty`, toggled from the context menu.
- [x] Continue-watching prompt near the centre controls with a 5-second countdown and resume/start-over actions.
- [x] OpenSubtitles REST client with API key/user agent, MD5 hash, search/download URL handling and offline/rate-limit/unauthorised error mapping (unit-tested pure parts; no live API call in tests).
- [ ] OpenSubtitles search/download UI dialog and API-key entry screen.
- [ ] Music mode with large cover art and albums/artists views; per-episode manual subtitle matching.

## Phase 6 — settings sidebar & presets

### Automated

```powershell
powershell -NoProfile -File scripts/cargo.ps1 fmt --all
powershell -NoProfile -File scripts/cargo.ps1 clippy --workspace --all-targets -- -D warnings
powershell -NoProfile -File scripts/cargo.ps1 test --workspace
powershell -NoProfile -File scripts/build-local.ps1 -Action Build
```

Latest validation: **71 tests passing** (60 core including 2 preset tests, 5 wrapper, 6 i18n), Clippy clean, release build succeeds. All new libVLC symbols were verified against the 3.0.x headers and the bundled DLL's export table.

### Preset auto-apply

- [x] Presets persist as TOML in SQLite with a typed payload (subtitle/audio/video/playback).
- [x] Auto-apply resolves the longest matching folder target for the current episode; applied on `start_item`, so the next episode in the same folder inherits it (unit-tested).
- [x] Save / load / delete / rename / auto-apply commands are wired to the worker and libVLC (video adjust, EQ preset, rate, delays, aspect).
- [x] Right sidebar with four tabs (Subtitles, Audio, Video, Playback) and preset panel, rendered in `docs/compare/player-settings.png`.
- [ ] Live preview thumbnails for video-filter changes, draggable subtitle-timeline sync, "Add subtitles" dialog with auto-matching and per-episode manual matching.

## Application icon

`assets/app-icon.ico` (original artwork supplied by the project owner, 256×256) is embedded into every `.exe` by `crates/app/build.rs` via `winres`, and bound to the Slint window `icon` property for player, mini player, library and gallery. Verified with `ExtractAssociatedIcon` on the three release binaries. The staged SDK's `rc.exe` path is supplied by `scripts/cargo.ps1` / `scripts/build-local.ps1`; no system-wide SDK install is required.

## Phase 5 — tracks, thumbnails, subtitles

### Automated

```powershell
powershell -NoProfile -File scripts/cargo.ps1 fmt --all
powershell -NoProfile -File scripts/cargo.ps1 clippy --workspace --all-targets -- -D warnings
powershell -NoProfile -File scripts/cargo.ps1 test --workspace
powershell -NoProfile -File scripts/cargo.ps1 build --release -p player-app --bin player
```

Latest validation: **69 tests passing** (58 core including 5 subtitle parser tests and 3 thumbnail-cache tests, 5 wrapper, 6 i18n), Clippy clean, release build succeeds.

### Subtitle smoke

```powershell
.\target\release\player.exe .\media\h264_1080p60.mp4 --sub-file docs\demo\sample.srt
```

- [x] SRT, WebVTT and basic ASS/SSA parse into a cue list with timing and style data (unit-tested); UTF-8/UTF-16 BOM and byte fallback decoding.
- [x] Own Slint subtitle renderer draws active cues with font size, colour, stroke/outline and vertical position; delay is applied at cue-selection time.
- [x] Live controls without restart: `[`/`]` delay ±100 ms, `-`/`=` size, `v` cycles position, `u` loads `PLAYER_SUBTITLE`.
- [x] Audio/subtitle track popovers from the control bar; video tracks remain in the sidebar/context menu.
- [x] Seek thumbnails use a separate headless libVLC player, an LRU cache and latest-wins cancellation; the main player is untouched.
- [x] Unsupported/bitmap formats fall back to libVLC slave rendering.
- [x] Live capture `docs/compare/player-subtitles.png` inspected visually.
- [ ] Real mouse-hover thumbnail capture in an automated test (no synthetic pointer injection yet).
- [ ] Full ASS/SSA style override mapping and subtitle-file drag & drop (Slint has no public file-drop hook).

## Phase 4 — queue & library

### Automated

```powershell
powershell -NoProfile -File scripts/cargo.ps1 fmt --all
powershell -NoProfile -File scripts/cargo.ps1 clippy --workspace --all-targets -- -D warnings
powershell -NoProfile -File scripts/cargo.ps1 test --workspace
powershell -NoProfile -File scripts/cargo.ps1 build --release -p player-app --bin library
```

Latest validation: **61 tests passing** (50 core including 3 new scanner tests, 5 wrapper, 6 i18n), Clippy clean, release build succeeds.

### Library smoke

```powershell
.\target\release\library.exe .\media
```

- [x] Cancellable incremental folder scanner upserts media by URI and links it to the folder (3 unit tests: recursive, non-recursive, rescan/cancel).
- [x] Sidebar sections, toolbar (back/forward, breadcrumb, search, sort, grid/list), media grid, queue panel with move/remove/clear, mini-player strip.
- [x] Add-to-queue, search and sort operate on persisted state; open-media launches `player.exe`.
- [x] Scans of 10k+ paths use a bounded stack and progress callbacks; the UI list is model-backed (true virtualisation still pending).
- [x] Live capture in `docs/compare/library-dark-en.png`, inspected visually.
- [ ] Thumbnail cache, watch/unwatch toggles in the UI, network-stream URL import, playlist CRUD dialogs, true list virtualisation, drag & drop reordering with the mouse (buttons work).

## Phase 3 — player view (completed pass)

- [x] Custom title bar using `no-frame` + `WindowMoveArea`, with working minimise / maximise-restore / close and Windows snap-drag.
- [x] Volume control is now a clipped cone shape.
- [x] Keyboard shortcut automation harness `scripts/test-shortcuts.ps1` sends WM_KEYDOWN/UP and captures the result (`docs/compare/shortcut-mute.png`); PostMessage delivery to Slint's FocusScope is not fully reliable, so real-user keyboard QA is still advised.

## Phase 3 — player view

### Automated

```powershell
powershell -NoProfile -File scripts/cargo.ps1 fmt --all
powershell -NoProfile -File scripts/cargo.ps1 clippy --workspace --all-targets -- -D warnings
powershell -NoProfile -File scripts/cargo.ps1 test --workspace
powershell -NoProfile -File scripts/cargo.ps1 build --release -p player-app --bin player
```

Latest validation: formatting/Clippy clean, **56 tests passing** (49 core, 5 wrapper, 2 i18n), release build succeeds.

### Player smoke

```powershell
$env:LIBVLC_PATH = (Resolve-Path 'third_party/vlc-3.0.24').Path
.\target\release\player.exe .\media\h264_1080p60.mp4
```

- [x] Real video renders with auto-hiding top/bottom overlay bars; the bars reappear when the pointer approaches the top/bottom edge.
- [x] Bottom bar: enlarged play between previous/next on the left, seek bar with time tooltip, volume cone, audio/subtitle/settings/sidebar/fullscreen on the right (matches `design/Player-2.png` layout intent).
- [x] Top bar: menu, title, mini-player and fullscreen controls; native Windows title bar retained so snap/resize/maximize keep working.
- [x] Right-click context menu with "Hide UI"; click-to-play, double-click fullscreen, mouse-wheel volume.
- [x] Shortcut map: Space/K play-pause, arrows seek/volume, F fullscreen, M mute, P mini player, N/B next/previous, O open, Escape hides UI. OSD shows the key/action.
- [x] Arabic RTL mirrors both bars and all control positions; light theme resolves overlay colours from tokens.
- [x] Live captures in `docs/compare/` inspected visually.
- [ ] Pixel-perfect match to Figma (control bar height/glyph shapes/volume cone styling still differ).
- [ ] Real keyboard-input automation and multi-monitor/DPI matrix (no public Slint scale-factor override).
- [ ] File drag & drop into the window and a native open-file dialog (Slint has no public file-drop hook; dialog is Phase 9).

## Phase 2 — design system (foundation)

### Inputs extracted from the Figma source

The supplied `VLC Redesign (Community).fig` was decoded with `openfig-cli`; see [`design/tokens.md`](../design/tokens.md) for the method and exact values. Outputs: `design/tokens.json`, `design/tokens.md`, `design/strings.json`. `design/tokens.md` is now the missing source of truth referenced by `CLAUDE.md`.

### Automated

```powershell
powershell -NoProfile -File scripts/cargo.ps1 fmt --all
powershell -NoProfile -File scripts/cargo.ps1 clippy --workspace --all-targets -- -D warnings
powershell -NoProfile -File scripts/cargo.ps1 test --workspace
powershell -NoProfile -File scripts/cargo.ps1 build --release -p player-app --bin gallery
```

Latest validation: formatting/Clippy clean, **54 tests passing** (49 core including 2 new i18n tests, 5 wrapper), release build succeeds.

### Gallery smoke test

```powershell
$env:LIBVLC_PATH = (Resolve-Path 'third_party/vlc-3.0.24').Path
.\target\release\gallery.exe                       # dark / English / orange
.\target\release\gallery.exe --light --lang ar     # light / RTL
.\target\release\gallery.exe --accent '#8A38F5'    # runtime accent override
```

- [x] Window opens with no stderr in dark, light, Arabic RTL and custom-accent configurations (launch smoke, 4 s, process alive).
- [x] Theme tokens come only from `design/tokens.json`; no literal colours in gallery/components.
- [x] Locale switch rewrites every `Strings` global property and sets RTL mirroring via `FlexboxLayout.flex-direction`.
- [x] Inter + Noto Sans Arabic are embedded at Slint compile time from `crates/app/ui/fonts/` with their OFL licences.
- [x] 42 SVG icons generated by `scripts/generate-icons.mjs` (30 mono control icons, 12 two-tone menu icons), five of them rendered in the gallery.
- [x] Scrollbar and breadcrumb components added; focus rings added to AppButton, Toggle, NumericSlider, SeekBar, VolumeCone and ListRow.
- [x] Real window captures in `docs/compare/` (dark/EN and light/AR), inspected visually — see [comparison notes](compare/README.md).
- [ ] Pixel-perfect composite comparison and the 100/125/150/200% DPI matrix (no public Slint scale-factor override; needs display-scale changes between runs).
- [ ] Focus/disabled state captures for every remaining component.

## Phase 1 — core engine

### Automated

```powershell
powershell -NoProfile -File scripts/cargo.ps1 fmt --all
powershell -NoProfile -File scripts/cargo.ps1 clippy --workspace --all-targets -- -D warnings
powershell -NoProfile -File scripts/cargo.ps1 test --workspace
powershell -NoProfile -File scripts/cargo.ps1 build --release -p player-app
```

Latest validation: formatting clean, Clippy clean with `-D warnings`, **52 tests passing** (47 core, 5 wrapper), release build succeeds. Wrapper ABI declarations for every newly used 3.0.x symbol were checked against the official `3.0.x` headers and confirmed exported by the bundled `libvlc.dll`.

Core tests cover: settings defaults/round-trip/clamping/change notifications, SQLite schema and CRUD for media/playlists/queue/progress/bookmarks/presets, queue ordering/shuffle/repeat/reorder, playlist sort/search, player-state transitions, A–B repeat detection, session auto-advance/resume, CLI parsing, file-association descriptor and single-instance hand-off.

### Headless engine harness

```powershell
$env:LIBVLC_PATH = (Resolve-Path 'third_party/vlc-3.0.24').Path
.\target\release\engine.exe .\media\h264_1080p60.mp4 .\media\h264_2160p30.mp4 --max-seconds 3
```

It maps libVLC events into the core state machine on a worker-style loop, prints every state transition, runs several files in sequence, applies A–B seeks and saves progress. Latest run: `Opening → Playing → Ended` for item 1, then the same for item 2, `ENGINE_DONE items=2`, exit 0.

- [x] Multiple files play in order and the queue advances on `EndReached`.
- [x] Per-media progress, recents and the queue survive in `%APPDATA%/PlayerApp/library.sqlite3`.
- [x] `resume_position()` restores an incomplete, non-watched position (unit-tested).
- [x] External subtitle/track/delay/snapshot/chapter calls are exposed in the wrapper and compile against the real DLL.
- [ ] Real-time clock in the headless harness: the synthetic clips are video-only and the tiny 320×180 frame sink reported `time=0` after 3 s, so the capped-run path saved position 0. A real vout/aout in the UI phase is needed to certify audible sync and visible resume.
- [ ] SMTC metadata publishing and global media-key hotkeys (deferred; needs an approved `crates/platform` module for the WinRT/Win32 FFI).
- [ ] Drag & drop into the window and file-association activation end-to-end (UI phase; descriptor + registry writer are implemented and unit-tested).
- [ ] Real file dialogs, permission errors and network streams (Phase 9/11).

### Known Phase 1 gaps

The app name is still the `<APP_NAME>` placeholder from `brand.rs` and must be confirmed before packaging. `design/tokens.md` and `design/SPEC.md` are still absent; Phase 2 needs them. Phase 0's 4K performance gate remains **FAILED** — no NV12/GPU optimisation spike has been authorised.

## Phase 0 testing

This checklist covers only the architecture spike, not the production screens.

## Automated

```powershell
powershell -NoProfile -File scripts/build-local.ps1 -Action Verify
```

This runs `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` with the process-local MSVC/SDK environment. Latest validation passed: release build, formatting, Clippy with denied warnings, and 14 unit tests (9 core, 5 wrapper). All five PowerShell scripts parsed successfully.

Wrapper tests cover C struct layout, 32-byte buffer alignment, exclusive reader ownership, bounded backlog, stale-frame rejection and dimension validation. Core tests cover time formatting, unknown/zero duration, progress clamping, counter resets and nearest-rank p95.

## Desktop smoke test

The two generated clips already exist in `media/`. The generator refuses overwrites; use a new output directory to regenerate. Build release, then run:

```powershell
powershell -NoProfile -File scripts/smoke-ui.ps1
powershell -NoProfile -File scripts/smoke-ui.ps1 -CloseCases
```

The script finds the visible titled window belonging to its own launched PID, sends pointer messages only to that HWND, and captures eight player-only screenshots with `PrintWindow`. It does not activate another application, send global keys, move the desktop cursor or copy desktop pixels. Its test thread uses physical per-monitor-DPI coordinates and restores the previous context. Latest smoke run exited 0; screenshots were visually inspected, not inferred from the exit code. The 125% DPI window capture is 1618×947 physical pixels; fullscreen is 1920×1080.

- [x] Moving video has correct red/blue order and opaque pixels.
- [x] Translucent HUD/control panels blend over live video, including fullscreen.
- [x] Pointer pause/resume; paused video-region pixels unchanged between two captures 300 ms apart.
- [x] Pointer timeline seek changes elapsed time (approximately 4 s → 15 s).
- [x] Language button mirrors HUD/control positions; Arabic labels visibly shape.
- [x] Pointer fullscreen toggle restores the window.
- [x] Pointer stop resets the timeline; play restarts from the beginning.
- [x] Timeline click and volume drag work; volume moved from 75% to 25%, fill left-anchored.
- [x] Automatic benchmark exit shuts down the playback worker successfully in the smoke run.
- [ ] Real Space/Left/Right/M/F/L/S keyboard input and focus behavior. Pointer messages do not certify these shortcuts.
- [ ] Audible mute/volume. Synthetic clips have no audio; the filename `03-playing-rtl-muted.png` is not evidence of sound or mute state.
- [x] Owned-window close while Playing/Paused/Stopped; corresponding events confirmed, graceful exit 0 in all three cases.
- [x] Explicit missing runtime: in-window error was captured and inspected, automatic benchmark exit 1, no Windows missing-DLL startup popup.

`-CloseCases` runs these four cases separately, checks event logs/exit codes, and adds [missing-runtime capture](demo/09-missing-runtime.png). It uses WM_CLOSE only on each process's verified owned window, not global input.

## Performance

Run each benchmark separately with the window visible and unobscured. Do not run encoding, compilation or another benchmark concurrently.

```powershell
powershell -NoProfile -File scripts/measure.ps1 -Media media/h264_1080p60.mp4 -Size 1920x1080
powershell -NoProfile -File scripts/measure.ps1 -Media media/h264_2160p30.mp4 -Size 3840x2160
```

GPU engine utilization is process-specific, maximum per engine, not a sum. CPU is normalized to all logical processors. Rendering notifier counts callback-sequence images reaching AfterRendering before presentation, not physical monitor scanout or content-hash uniqueness; pause can produce callbacks with repeated pixels. The application starts its three-second warmup after renderer setup, not after the first frame, while the external sampler drops its first three process samples. The two measurement windows are not identical.

Two 20-second runs completed with valid CPU/frame reports; both had 0/20 valid GPU samples. A retry with refreshed wildcard queries still had no valid GPU samples and timed out; its incomplete results are excluded. Foreground/occlusion were uncontrolled in this desktop session. See [results and failed stop gate](spike.md#performance-method-and-results). GPU utilization and repeatable unobscured-window acceptance measurements remain open. Synthetic H.264 does not certify HEVC/AV1/HDR/network playback.

## Deferred to production phases

100/125/150/200% DPI, multi-monitor moves, snap behaviors with custom title bar, keyboard accessibility/focus states, bundled fonts, pixel comparisons, audio/subtitle sync, persistence, SMTC and installer/file associations are not certified by this spike.
