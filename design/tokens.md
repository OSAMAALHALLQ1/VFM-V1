# Design tokens — extracted from `VLC Redesign (Community).fig`

Source of truth for visuals. This file was generated from the Figma file supplied on 2026-11-20 (its own metadata records export `2026-11-20T11:08:12.872Z`), not guessed from the PNGs. If a token below conflicts with a screenshot, the Figma node tree wins; if behaviour conflicts with a screenshot, `SPEC.md` wins.

## Extraction method (reproducible)

The `.fig` is a ZIP containing `canvas.fig` (`fig-kiwi` wire format), `meta.json`, a thumbnail and 44 embedded images. It was decoded with the open-source [`openfig-cli`](https://github.com/OpenFig-org/openfig-cli) (MIT):

```powershell
npm install --prefix "$env:TEMP\figtools" openfig-cli
& "$env:TEMP\figtools\node_modules\.bin\openfig.cmd" inspect 'C:\Users\M.S.I\Downloads\VLC Redesign (Community).fig'
& "$env:TEMP\figtools\node_modules\.bin\openfig.cmd" list-text 'C:\Users\M.S.I\Downloads\VLC Redesign (Community).fig'
```

The decoded scene has **11,613 nodes**, **1,945 TEXT nodes** and **1,313 blobs**. 69 named colour swatches (`Dark/*`, `Light/*`) were found with their exact fills; radii and spacing are frequency-ranked from frame/vector properties; typography is counted over all text nodes. The machine-readable result is [`tokens.json`](tokens.json). The full hierarchy dump and text dump were kept outside the repository in the agent temp directory.

## Colour palette

Dark theme — names are verbatim Figma swatch names:

| Token | Hex | Usage in Figma |
| :--- | :--- | :--- |
| `dark.accent.main` / `dark.accent.logo` | `#D37918` | Accent, logo fill, selected states (260 uses) |
| `dark.accent.bright` | `#FC9729` | Accent hover/bright |
| `dark.accent.dim` | `#AB6314` | Accent dim, thumb |
| `dark.bg.a1` | `#222222` | Elevated surfaces |
| `dark.bg.a2` | `#1F1F1F` | Layer 2 |
| `dark.bg.a3` | `#1B1B1B` | Miniplayer / expanded panels |
| `dark.bg.a4` | `#191919` | Controls / topbar (382 uses) |
| `dark.bg.a5` | `#151515` | Deepest layer |
| `dark.bg.accent-bright` | `#371F06` | Accent-tinted surface |
| `dark.bg.accent-dark` | `#1A1611` | Accent-tinted panel (979 uses) |
| `dark.bg.b1` | `#0E0E0E` | Primary background (settings, onboarding) |
| `dark.bg.b2` | `#090909` | Secondary background |
| `dark.fg.1` | `#EAEAEA` | Primary text |
| `dark.fg.2` | `#C6C6C6` | Secondary text |
| `dark.fg.a1` | `#969696` | Labels / keycap hints |
| `dark.fg.a2` | `#8F8F8F` | Icons / tertiary text (1,449 uses) |
| `dark.fg.a3` | `#6B6B6B` | Muted text (817 uses) |
| `dark.fg.a4` | `#404040` | Placeholder text |
| `dark.divider.bright` | `#373737` | Bright separator |
| `dark.divider.dim` | `#222222` | Dim separator |

Light theme — verbatim swatch names:

| Token | Hex |
| :--- | :--- |
| `light.accent.main` | `#E8851A` |
| `light.accent.bright` | `#FFA13B` |
| `light.accent.dim` | `#965916` |
| `light.accent.logo` | `#C2711A` |
| `light.bg.a1` | `#EDEDED` |
| `light.bg.a2` | `#DDDDDD` |
| `light.bg.a3` | `#D5D5D5` |
| `light.bg.a4` | `#B3B3B3` |
| `light.bg.a5` | `#888888` |
| `light.bg.accent-bright` | `#D8BA9A` |
| `light.bg.accent-dark` | `#C6AB89` |
| `light.bg.b1` | `#E7E7E7` |
| `light.bg.b2` | `#DEDEDE` |
| `light.fg.1` | `#EAEAEA` |
| `light.fg.2` | `#C6C6C6` |
| `light.fg.a1` | `#3C3C3C` |
| `light.fg.a2` | `#2E2E2E` |
| `light.fg.a3` | `#6B6B6B` |
| `light.fg.a4` | `#3B3B3B` |
| `light.fg.b1` | `#161616` |
| `light.divider.bright` | `#9B9B9B` |
| `light.divider.dim` | `#ACACAC` |

> Note: the light `fg.1`/`fg.2` swatches inherit the dark values in the source file. They are reproduced faithfully here; the theme module may remap them if a light-screen screenshot proves otherwise.

## Typography

- Latin: **Inter** (1,936 of 1,945 text nodes); monospaced timecodes: **DM Mono** (9 nodes).
- Arabic: **Noto Sans Arabic** (bundled in Phase 2; not present in the Figma file, which uses Inter for its Arabic placeholder glyphs).
- Sizes (px, ranked by use): `8, 10, 12, 14, 15, 16, 20, 24, 32`. Dominant: **14 px** (1,451 nodes), then 16 px (306), 12 px (139).
- Weights: `300, 400, 500, 600, 700`. Dominant: **400** (1,694), then 500 (96), 600 (83), 300 (71).
- Body = 14/400, caption = 12/400, title = 16/600. Line height is 100% at the sampled nodes.

## Spacing & radii

- Spacing scale (px): `2, 4, 6, 8, 10, 12, 14, 16, 20, 24, 32`. Most frequent: 10 (4,720), 4 (3,098), 2 (3,081), 8 (1,733), 5 (1,162), 20 (744).
- Radii (px): `4, 6, 12, 14, 19, 100`. `4` and `6` dominate controls; `12–19` are panels/cards; `100` is pill/circular. A 4 px outside stroke with 19 px radius appears on the large control buttons (`Frame 151`, 116×116).

## Elevation & motion

- Background blur radii: **15 px** (111 uses), **50 px** (9 uses), 27.5 px (9 uses).
- Drop-shadow radii: **11.8 px** (28 uses), **13.8 px** (12 uses), 15 px (38 uses); a few 90.5 px ambient shadows.
- Motion curves/durations are **not encoded** in a static Figma file. They must be defined in `SPEC.md` before the animation tokens are finalised; until then the theme module exposes no motion constants.

## Screens present in the file

The canvas contains (among others): `Player` (1920×1080, dark), `Player - Light`, `Player - Music`, `Miniplayer`, `Player - hide UI`, `Player - customize`, `Controls`, `Button`, `Toggle`, `volume`, `sidebar`, `context menu`, `right-click menu`, `Settings - global/interface/privacy/playback/video/audio/subtitles/controls/library/network/performance/recording/extensions`, `Audio - Equalizer/Compressor/Spatializer/Stereo Widener`, `Video - Essential/Colors/Crop`, `Onboarding` (11 frames), `Keyboard Shortcuts`, `Profiles`, `Add Subtitles to Playlist/Folder`, `Open network stream`, `new playlist`. This confirms the Phase 2–10 screen inventory.

## Compliance note

The Figma file still contains a `name=vlc` cone icon and text such as "Welcome to VLC!". Per `CLAUDE.md`, those are **not** reproduced: the app name stays the single `brand.rs` constant and the cone is replaced by an original logo. The orange accent and the cone-shaped volume slider shape are permitted by the brief.
