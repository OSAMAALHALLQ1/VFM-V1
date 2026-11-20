# Figma comparison captures

Source Figma file: `VLC Redesign (Community).fig` (exported 2026-11-20). Token extraction and method: [`../design/tokens.md`](../design/tokens.md). PNG exports from Figma are in `design/`.

## Captures

| Capture | Configuration | Command |
| :--- | :--- | :--- |
| [gallery-dark-en.png](gallery-dark-en.png) | Dark, English, orange accent | `powershell -NoProfile -File scripts/capture-gallery.ps1 -Name gallery-dark-en` |
| [gallery-light-ar.png](gallery-light-ar.png) | Light, Arabic RTL, orange accent | `powershell -NoProfile -File scripts/capture-gallery.ps1 -Name gallery-light-ar -GalleryArgs '--light --lang ar'` |
| [player-dark-en.png](player-dark-en.png) | Player, dark, English, live 1080p video | `powershell -NoProfile -File scripts/capture-player.ps1 -Name player-dark-en -GalleryArgs '.\media\h264_1080p60.mp4'` |
| [player-light-ar.png](player-light-ar.png) | Player, light, Arabic RTL | `$env:PLAYER_LOCALE='ar'; $env:PLAYER_LIGHT='1';` then the capture command above with `-Name player-light-ar` |

Both captures are 1418×1022 physical pixels at this desktop's current scale and were inspected visually. They show that the palette, type scale, radii, states, RTL mirroring and bundled fonts render; the gallery is a component review harness, not a pixel copy of a Figma screen.

## Still open

- Side-by-side composite images against the corresponding Figma frames, including the 100/125/150/200% DPI matrix. Slint 1.19 exposes no public scale-factor override, so this requires either changing the Windows display scale between runs or adding a test-only renderer hook.
- Component-level focus/disabled captures for every component state.
