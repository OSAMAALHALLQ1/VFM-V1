# Project: <APP_NAME> — a custom Windows media player (Rust + libVLC 3.0.x + Slint)

## Sources of truth
- design/*.png (Figma exports), design/tokens.md, design/SPEC.md (product/UX spec). If they conflict: SPEC.md for behavior, PNGs for visuals. Ask when ambiguous.

## Architecture decisions (do not change without asking)
- Workspace: crates/vlc-wrapper (all unsafe, libloading, runtime-resolved libvlc.dll), crates/core (state, playlist, library, settings, no unsafe), crates/app (Slint UI).
- Video: libVLC frame callbacks -> double buffer -> Slint image (no HWND, no airspace). NV12+wgpu path only if benchmarks demand it.
- Never call libVLC from inside lock/unlock/display callbacks. UI thread never blocks on libVLC.
- Branding: name/exe/app ID defined in ONE brand module. No VLC name or cone logo anywhere (own logo). Orange accent and cone-shaped volume slider are fine.
- Languages: English + Arabic with full RTL mirroring. All UI strings through an i18n layer (no hard-coded strings).
- Persistence: SQLite (library, bookmarks, presets, watch progress), TOML/JSON for settings.

## Rules
- Small commits, one feature per commit. `cargo fmt`, `clippy -D warnings`, `cargo test --workspace` must pass before each commit.
- Styling only through the theme module (tokens). No magic numbers/colors in screens.
- Unit-test core logic. Keep a manual test checklist in docs/testing.md and update it each phase.
- End each phase with: summary, screenshots vs Figma in docs/compare/, known issues, exact run commands. Then STOP and wait.
- Out of scope: reverse playback (libVLC can't), anything not in SPEC.md.
