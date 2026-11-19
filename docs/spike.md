# Phase 0 — libVLC 3.0.x + Slint, Windows x64

Status: functional Phase 0 proof validated; performance gate **FAILED**, GPU utilization measurement **unresolved**. This is an architecture prototype, not a production player. Phase 1 has not started.

## Scope and inputs

The supplied Figma export directory contained 25 PNGs, copied unchanged into `design/`. There was no `tokens.md`. The application name is still `<APP_NAME>` pending confirmation. The prototype title is **Player — Phase 0**, binary `spike.exe`; no cone or VLC product branding is used. Technical engine names and required DLL filenames remain in source/documentation.

The later attached design narration clarifies intent: compact rectangular controls, unobtrusive overlays, discoverable tracks/library/queue and simpler settings. It does not authorize implementing those later-phase features. Its request to retain the cone conflicts with the explicit no-branding rule, so the no-branding rule governs.

The attached outline was treated as a proposal, not measured evidence: this workspace is on Windows with working Rust/MSVC, not the Linux container described there. Its RGBA and performance claims were not assumed true. Phase 0 deliberately prototypes only the requested **RV32/BGRX → double buffer → Slint image** path. Design colors/layout are provisional; pixel accuracy, bundled fonts and production RTL are later-phase acceptance work.

## Architecture and wrapper choice

```
Cargo.toml                 workspace, resolver 3, edition 2024
crates/vlc-wrapper         runtime DLL loader, hand-audited 3.0.x ABI, RAII player/media, event channel, aligned video buffers
crates/core                platform-independent playback state, time and telemetry math
crates/app                 Slint overlay UI, command worker, release benchmark
scripts                    runtime fetch, clip generation, CPU/GPU measurement, desktop smoke captures
```

`vlc-rs` 0.3.0 (published 2018) and `libvlc-sys` 0.2.0 (2019) were evaluated. They add native linking/import-library integration; the latter brings its old binding/build stack. Selected: a deliberately small, reviewed 3.0.x C ABI loaded with `libloading` 0.9.1. This removes libclang/bindgen and MSVC `.lib` generation from this spike and allows a missing runtime to become an application error rather than a Windows startup loader failure. ABI declarations were checked against VideoLAN's `include/vlc/libvlc*.h`; tests check the used x64 struct offsets. Maintaining this handwritten ABI is a tradeoff, not a claim that it covers the whole API.

All handwritten Rust `unsafe` is in `vlc-wrapper`; `core` forbids it and `app` denies it. Slint's generated item-tree code requires internal unsafe glue, confined to `app`'s generated UI module with a narrowly scoped lint exception; no handwritten unsafe is added there. The player retains DLLs and callback state until after stop/release. The installed callback format is `libvlc_video_set_format("RV32", width, height, width*4)`. `--size` must equal clip dimensions for a native-resolution benchmark; libVLC otherwise scales. Automatic format negotiation is deferred.

## Buffer ownership, composition and threading

Two fixed, 32-byte-aligned pixel allocations use atomic ownership states:

```
FREE → WRITING (libVLC lock/copy) → READY (display) → READING (UI lease) → FREE
```

A CAS grants exclusive pixel ownership; release/acquire publishes complete frames. `lock` hands libVLC a stable pointer; `display` publishes after the copy and frees the picture token. The UI obtains a nonblocking read lease and copies/swizzles BGRX to Slint `SharedPixelBuffer<Rgba8Pixel>`, explicitly setting alpha to 255. A consumption watermark prevents an older ready frame from appearing after a newer one. An unread ready frame can be replaced, counted as a drop. If both slots are unavailable, libVLC writes a private discard buffer rather than blocking or overwriting a reader. This rare fallback is separately counted; it does not grow the queued-frame backlog.

An Image is drawn inside the Slint scene; translucent HUD and controls are ordinary scene elements over that image. There is no child video HWND, so the native-window airspace issue is avoided. The native Windows title bar is retained for this prototype.

The UI polls frames with a 4 ms Slint timer; no per-frame closures accumulate. Conversion/upload happens on the UI thread, which is a measured cost and potential bottleneck, not “free”. A dedicated worker owns libVLC and processes play/pause/stop/seek/volume/mute commands. LibVLC event callbacks only copy event payloads to an asynchronous `mpsc` channel; the worker drains it, then supplies UI snapshots through a bounded channel. No callback calls libVLC. Stop/release join the vout threads on the worker. Video installation also stops/joins before configuring callbacks, preventing unsafe late-install races.

The bridge was checked against 3.0.x `modules/video_output/vmem.c` and `src/video_output/video_output.c`: Prepare invokes lock/copy, Display invokes the picture callback; RV32 on little-endian Windows is **B,G,R,unused**, not usable alpha. Snapshot queries are ~200 ms apart, so libVLC statistics at measurement boundaries have up to one query interval of uncertainty.

## Obtaining and locating the runtime

Verified official artifact: [VideoLAN 3.0.24 win64 zip](https://download.videolan.org/pub/videolan/vlc/3.0.24/win64/vlc-3.0.24-win64.zip).

SHA-256: `fcf30850371ad10c9373cc4f0f4501e7dee49e3e9ae9f20c72fb2661a1ca6323`, matched against the official `.sha256` and local archive.

```powershell
powershell -NoProfile -File scripts/fetch-libvlc.ps1
```

The already-extracted local runtime is `third_party/vlc-3.0.24`. The fetch script refuses to overwrite an existing extracted directory and generates the official plugin cache after a fresh extraction. This zip shipped without `plugins/plugins.dat`: an independent native initialization took **77.6 seconds** scanning 493 modules. Running `third_party/vlc-3.0.24/vlc-cache-gen.exe third_party/vlc-3.0.24/plugins` generated the cache without changing DLLs, drivers or system settings. Do not diagnose that cold scan as a confirmed deadlock. The renderer is shown before the playback worker starts to avoid overlapping their first initialization. This official zip **does not contain an SDK directory**. Headers for auditing are available in [VideoLAN's 3.0.x source](https://code.videolan.org/videolan/vlc/-/tree/3.0.x/include/vlc); the runtime-loaded build does not need SDK headers, bindgen, clang or import libraries. Do not substitute 32-bit DLLs or a 4.x nightly.

Runtime search order: `LIBVLC_PATH` → `<exe>/vlc/` → `<exe>/` → `%ProgramFiles%/VideoLAN/VLC`. `--runtime DIRECTORY` is an explicit CLI override. The first DLL pair selected must contain `plugins/`; a broken selected bundle fails rather than silently switching versions. Libraries are loaded by absolute path using Windows DLL-directory + System32 search flags, core first. The loader accepts only `3.0.*`. Plugins resolve beside libvlccore using libVLC's Windows module-directory discovery, without mutating the process environment from multithreaded Rust. A caller-set `VLC_PLUGIN_PATH` can add search locations and should be cleared for controlled tests.

Portable runtime layout for later packaging:

```
spike.exe
vlc/
  libvlc.dll
  libvlccore.dll
  plugins/                 preserve complete tree/dependencies
```

`.cargo/config.toml` points cargo-run development sessions at the local extracted runtime; direct executable invocation needs the environment override or portable layout above.

## Build and run (PowerShell, project root)

Prerequisites: Windows 10/11 x64, Rust/MSVC stable **1.95+** (Slint 1.19's actual minimum), Visual Studio C++ Build Tools and Windows SDK. Tested toolchain: Rust/Cargo 1.98.0. Open a Developer PowerShell if MSVC tools are not discoverable.

This machine had MSVC 14.44 but no installed SDK libraries, plus a user-level `rust-lld` override. `scripts/build-local.ps1` stages Microsoft's official `Microsoft.Windows.SDK.CPP.x64` 10.0.26100.7705 NuGet package under ignored `third_party/windows-sdk-x64`, selects the installed MSVC linker, and sets LIB only in the build subprocess. No user Cargo configuration or system installation is modified. The package's recorded SHA-256 is `ad6ce90ff94010699d29548b3c2b1e52c89eb81143e2199155d9a5c38001b52b`. On this machine use:

```powershell
powershell -NoProfile -File scripts/build-local.ps1 -Action Verify
powershell -NoProfile -File scripts/build-local.ps1 -Action Build
$env:LIBVLC_PATH = (Resolve-Path 'third_party/vlc-3.0.24').Path
.\target\release\spike.exe .\media\h264_1080p60.mp4 --size 1920x1080
```

With a complete standard Developer PowerShell environment, the underlying Cargo commands are:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p player-app
$env:LIBVLC_PATH = (Resolve-Path 'third_party/vlc-3.0.24').Path
.\target\release\spike.exe 'C:\path\to\clip.mp4' --size 1920x1080
# Or cargo supplies the local runtime environment:
cargo run --release -p player-app -- 'C:\path\to\clip.mp4' --size 1920x1080 --rtl
```

Keyboard: Space play/pause, Left/Right ±5 s, M mute, F fullscreen, L language-preview toggle, S stop. Pointer seek/volume controls are live. Arabic mode mirrors HUD and control positions; media timeline/timecodes intentionally remain LTR. System Segoe UI is used for the spike; font bundling and comprehensive RTL/accessibility are not complete.

## Performance method and results

Release binary, 1280×720 logical window at this desktop's 125% DPI, source buffers at native resolution. The window was shown, but foreground/occlusion could not be controlled in this desktop session; these are exploratory results, not an unobscured-window acceptance benchmark. H.264/yuv420p synthetic moving/noisy 30-second clips: 1920×1080@60 (~25 Mb/s) and 3840×2160@30 (~50 Mb/s), software decode (`avcodec-hw=none`), winit/femtovg OpenGL. Runs were sequential, after builds/encoding finished. The clip generator uses libx264, not NVENC: installed NVIDIA driver supports NVENC API 11.1, while this ffmpeg build requires 13.0. No driver changes were made.

The application waits three seconds after renderer/notifier setup, then measures twenty seconds; this is **not** a warmup anchored to the first displayed frame, so startup stalls can remain in the measured interval. External CPU sampling excludes the first three process samples, not precisely the same application interval. CPU and frame statistics therefore have different boundaries. CPU summaries are arithmetic means of actual-interval samples, with twenty valid samples in each successful run.

```powershell
# Clips already exist locally; only regenerate into a new directory:
powershell -NoProfile -File scripts/generate-clips.ps1 -Output media-regenerated
powershell -NoProfile -File scripts/measure.ps1 -Media media/h264_1080p60.mp4 -Size 1920x1080
powershell -NoProfile -File scripts/measure.ps1 -Media media/h264_2160p30.mp4 -Size 3840x2160
```

### Successful 20-second measurements

Windows 11 Home 22631, i7-11800H (8C/16T), ~16 GB RAM. Installed adapters: RTX 3050 Ti Laptop (30.0.15.1165), Intel UHD (30.0.101.2079), and LuminonCore IDDCX. Adapter enumeration does not establish which adapter rendered the OpenGL scene.

| Metric | H.264 1080p60 | H.264 4K30 |
| :--- | :--- | :--- |
| Application interval | 20.019 s | 20.121 s |
| Display callbacks / rate | 1201 / 59.99 fps | 285 / 14.16 fps |
| Unique frames reaching AfterRendering / rate | 664 / **33.17 fps** | 92 / **4.57 fps** |
| Unread frames overwritten | 537 (44.7% of callback count) | 191 (67.0% of callback count) |
| Starved / supplied but not rendered | 0 / 1 | 0 / 1 |
| libVLC decoded / displayed / lost deltas | 2400 / 1200 / 0 | 791 / 276 / 93 |
| Allocate + swizzle + Image assignment, mean / p95 | 14.05 / 22.53 ms | 70.73 / 95.60 ms |
| Render gap, mean / p95 | 30.13 / 51.08 ms | 137.28 / 192.67 ms |
| Process CPU, mean / sample maximum (normalized 16T) | 4.59 / 8.37% | 9.21 / 24.21% |
| Working set, mean / maximum | 395.95 / 401.47 MiB | 742.53 / 1027.27 MiB |
| Valid process GPU samples | **0/20 — unavailable** | **0/20 — unavailable** |

Evidence, including actual sample intervals and raw counters:
- [1080p process report](bench/spike-20261119T210733174Z-1920x1080-6b8929b5.json), [application report](bench/spike-20261119T210733174Z-1920x1080-6b8929b5.spike.json), [CSV](bench/spike-20261119T210733174Z-1920x1080-6b8929b5.csv).
- [4K process report](bench/spike-20261119T210827663Z-3840x2160-0ba680fa.json), [application report](bench/spike-20261119T210827663Z-3840x2160-0ba680fa.spike.json), [CSV](bench/spike-20261119T210827663Z-3840x2160-0ba680fa.csv).

GPU limitations: those successful runs used a continuous query primed before launch and returned no matching process engines. The current harness refreshes wildcard instances per query to avoid excluding a subsequently launched PID. Its [1080p retry](bench/spike-20261119T211734591Z-1920x1080-e71cf9cc.json) still returned no valid GPU samples, reported an invalid PDH sample, and exceeded the 38-second process deadline without an application report. That **failed retry is excluded from the table**; the chained 4K retry did not run. GPU utilization and the cause of the retry stall remain unresolved. No GPU value or adapter selection is inferred, and no system counter/driver settings were changed.

Finding: callback delivery at 1080p approaches 60 fps, but only 33.17 fps reaches the renderer. At 4K, the 70.73 ms mean CPU allocation/conversion/assignment cost alone exceeds the 33.33 ms frame budget, while both delivery and rendering fall far below 30 fps. The UI-side path is a demonstrated bottleneck; these timings do not isolate decoder, upload, driver, or occlusion costs. The raw render-gap average excludes time before the first measured render and is not the reciprocal of whole-window rendered fps.

Definitions: `callbacks` counts vmem display callbacks, not decoder frames; `rendered_unique` counts unique supplied frames reaching Slint AfterRendering, not physical scanout/PresentMon. `overwritten_unread` and `starved` are bridge losses, `supplied_not_rendered` is UI coalescing, and `libvlc_lost` is libVLC's separate cumulative vout loss delta. These cannot be combined into one exact end-to-end percentage because sampling boundaries differ. Copy timing includes allocation + BGRX swizzle + Image assignment but excludes asynchronous GPU texture upload. CPU is process processor-time/wall-time divided by 16 logical processors. GPU is the maximum individual process GPU-engine counter per sample, not a sum. Missing GPU counters are reported as unavailable, never zero by assumption.

Theoretical pixel payload (not measured performance): 1080p60 = 8.29 MB/frame, 498 MB/s per copy; 4K30 = 33.18 MB/frame, 995 MB/s per copy. Both include additional conversion, upload and decoder costs.

## Optimization path and stop gate

**Gate decision: FAIL.** The 4K bridge overwrites 67.0% of the measured callback count and delivers only 4.57 rendered fps. This fails the >2% bridge-loss / poor-pacing gate even though mean normalized CPU is below 25%. GPU evidence is incomplete. Do not proceed to production screens on this path without a separately authorized optimization spike and repeatable, foreground-controlled measurements. Proposed steps, **not implemented or benchmarked**:

1. Request I420/NV12 through format callbacks with aligned plane pitches. Upload Y and UV planes (~12 rather than 32 bits/pixel), convert with a WGSL GPU shader, including explicit BT.601/709 range/color handling. This reduces CPU chroma conversion and upload bandwidth; it is not hardware zero-copy decoding.
2. Enable Slint 1.19 `unstable-wgpu-30` and a compatible renderer, share one WGPU device/queue with `BackendSelector::require_wgpu_30`, reuse ping-pong textures, import the output as `Image::try_from(wgpu::Texture)`. Actual 1.19 source accepts Rgba8Unorm/Rgba8UnormSrgb with TEXTURE_BINDING + RENDER_ATTACHMENT usage. Prove backend/device compatibility and texture lifetime in that spike.
3. libVLC 3.0's public callbacks expose CPU picture buffers, not D3D11 shared textures. Do not claim a public 3.0 zero-copy path. If it becomes mandatory, discuss a different engine or a separately scoped libVLC 4 migration; stable 3.0 remains this project's constraint.

**Stop after Phase 0.** Confirm the application name and supply `design/tokens.md` before Phase 1. Pixel-perfect screens, core feature set, persistence, Windows integrations, packaging/CI, fonts and installer branding are not delivered by this spike.

## Licensing and demo

libVLC core is LGPL-2.1-or-later; the full official plugin bundle contains components with additional/GPL obligations. Dynamic loading alone does not resolve distribution obligations. Slint 1.19 offers GPL-3.0-only / Slint Royalty-free 2.0 / commercial Software 3.0 licensing; choose a suitable application distribution license before packaging. No installer or redistributable release was created in Phase 0.

Demo: moving video inside one Slint window with translucent filename/performance HUD and transport overlay. Native owned-window tests verified pointer pause/resume, seek, volume drag, stop/restart, English/Arabic mirroring and fullscreen/restore. Eight player-only screenshots were inspected; two paused captures 300 ms apart had identical video-region pixels. The smoke run exited with code 0. Separate close tests while Playing/Paused/Stopped all exited 0 with matching events confirmed; an explicit missing-runtime case showed an in-window error and exited 1. Real keyboard/focus behavior, audible mute and the full DPI matrix are not certified.

- [Playing, English](demo/01-playing-ltr.png)
- [Paused overlay](demo/02-paused-overlay.png)
- [Arabic preview](demo/03-playing-rtl-muted.png) — historical filename, not proof of audible mute
- [Fullscreen composition](demo/04-fullscreen-overlay.png)
- [Volume dragged to 25%](demo/07-volume-pointer.png)
- [Stopped](demo/05-stopped.png), [restarted](demo/06-restarted.png), [pause confirmation](demo/08-paused-confirm.png)
- [Missing-runtime error](demo/09-missing-runtime.png)

```powershell
powershell -NoProfile -File scripts/smoke-ui.ps1
powershell -NoProfile -File scripts/smoke-ui.ps1 -CloseCases
```

Validation: release build, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and **14 unit tests passed** (9 core, 5 wrapper, including a 10,000-frame concurrent ownership stress test). All five PowerShell scripts parsed successfully. See [testing scope](testing.md) for remaining checks. No GPU optimization or Phase 1 implementation was started.
