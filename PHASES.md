# خطة التنفيذ — <APP_NAME>

> "فيزات" = المراحل. هذه الخطة الكاملة بترتيب مناسب للتنفيذ، وكل مرحلة جاهزة كبرومبت تعطيه للوكيل **وحدها** وبعد قبول نتيجة السابقة.
> المرحلة 0 هي الـ spike الذي بدأتَه، وإن اكتملت فاستمر من المرحلة 1.
> الملف الثابت `CLAUDE.md` موجود في جذر المشروع (يُكتب مرة واحدة ولا يتغير).

---

## المرحلة 0 — إنهاء الـ spike

```
Finish Phase 0 on this Windows machine: make the workspace compile, pass fmt/clippy/tests, render a real video with HUD overlaid, run the 1080p60 and 4K30 benchmarks (RGBA vs RV32), fill docs/spike.md with real numbers. If 4K30 >2% drops or >25% CPU, implement NV12 + GPU shader path. Review all unsafe code in vlc-wrapper. Stop.
```

**القبول:** فيديو يعمل بسلاسة 1080p، أرقام حقيقية، لا تحذيرات.

---

## المرحلة 1 — المحرك الأساسي (Core)

```
Phase 1 — Core engine. Implement in vlc-wrapper/core:
- Player: play/pause/stop, accurate seek/time/length, volume/mute, speed, audio/subtitle track enumeration + selection, audio delay, subtitle delay, aspect ratio, A–B repeat, frame step, snapshot, chapters.
- Typed events from libVLC -> async channel -> UI state (state machine in core with tests).
- Queue model (auto-play next, shuffle, repeat, reorder) separate from Playlist model (saved collections). Persist both.
- SQLite schema + migrations: media items, folders, playlists, queue, watch progress (resume position, watched flag), bookmarks, presets.
- Settings store (typed, defaults, versioned, TOML) + change notifications.
- Windows integration: CLI args/open-with, file associations helper, drag&drop, media keys + SMTC, single instance, remember window state.
Tests for queue/playlist/progress logic. No new UI beyond what's needed to test.
```

**القبول:** تشغيل ملفات متعددة متتالية، استئناف من آخر موضع، اختبارات تمر.

---

## المرحلة 2 — نظام التصميم والمكوّنات

```
Phase 2 — Design system. Generate a theme module from design/tokens.md (colors, type scale, spacing, soft radii, elevation, motion) with dark + light themes and a runtime-changeable accent color (default orange). Bundle Arabic+Latin fonts. Implement the i18n layer (en, ar) with RTL mirroring. Create the two icon sets (two-tone menu icons, monochrome control icons) as SVG assets. Build reusable components with all states: icon button, button, toggle, slider (numeric + thick seekbar variant + cone volume variant), dropdown, tabs, list row, card, grid item, context menu, tooltip, dialog, sidebar panel, scrollbar, search field, breadcrumb bar, toast/OSD. Provide a hidden "component gallery" window to review every component against Figma.
```

**القبول:** نافذة المعرض تطابق Figma في الوضعين وبالعربية RTL.

---

## المرحلة 3 — واجهة المشغّل

```
Phase 3 — Player view per SPEC.md: bottom control bar (play enlarged between prev/next on the left; queue, subtitles, audio, volume cone, settings/sidebar, fullscreen on the right), thick seek bar with time tooltip, top bar (menu, title, fullscreen, PiP, window controls) with show-on-hover-near-edge / hide-on-leave for both bars (also in fullscreen), right-click context menu (+ "Hide UI"), shortcut OSD that shows the key pressed + action, full keyboard shortcut map, fullscreen, mini player / PiP window, custom title bar keeping Windows snap/resize/maximize, DPI 100–200%, multi-monitor. Click-to-play, double-click fullscreen, mouse-wheel volume, drag-and-drop files into the window.
```

**القبول:** مطابقة لقطات الشاشة مع Figma، كل الاختصارات تعمل.

---

## المرحلة 4 — الـ Queue والمكتبة

```
Phase 4 — Queue & Library. 
- Queue panel (drag/drop, reorder, remove, now playing, clear) and "add to queue" everywhere.
- Library: Home (continue watching, recent files/playlists/streams), Videos, Music (songs/albums/artists/genres with cover art, tag reading via `lofty`), Network streams (paste URL or M3U/M3U8 -> auto playlist), Playlists (create/rename/delete/add from anywhere), Recent, Pinned sidebar items.
- Folder watching/scanning in background (incremental, cancellable), thumbnails cache, watched/unwatched tracking.
- Toolbar: back/forward, clickable breadcrumb address bar, view modes (grid/list/detail), sort (name/date/duration/size/unwatched-first/watched-first), search, overflow menu (add file/folder).
- Compact library sidebar inside the player (drag items to queue) and full-screen library view with mini player.
- Handle 10k+ items with virtualized lists.
```

**القبول:** إضافة مجلد مسلسل واستعراضه وتشغيله دون مغادرة الفيديو.

---

## المرحلة 5 — المسارات ومعاينة الشريط والترجمة

```
Phase 5 — Tracks, seek thumbnails, subtitles.
- Subtitle and audio popovers from the control bar (track lists; audio also lists output devices), video tracks via sidebar + context menu only.
- Seek-bar hover thumbnails using a separate headless libVLC player/frame grabber with caching and cancellation; must not affect main playback.
- Own subtitle renderer in Slint: SRT + WebVTT first (parse, timing, styling: font, relative/precise size, outline, color, position, delay), then ASS/SSA basic styling. Fall back to libVLC rendering for unsupported/bitmap formats.
- External subtitle loading (drag&drop, file picker), encoding detection.
- Subtitle timeline data model (cue list with start/end) for Phase 6.
```

**القبول:** تغيير حجم/موضع/تأخير الترجمة حياً بدون إعادة التشغيل.

---

## المرحلة 6 — الشريط الجانبي (4 تبويبات) والـ Presets

```
Phase 6 — Right sidebar per SPEC.md.
- Subtitles tab: delay slider, font/size/outline/position, relative vs precise sizing, default track/language, subtitle timeline (drag to shift sync), "Add subtitles" dialog with target (file/folder/playlist) and auto-matching with human-readable match modes (dropdown labels, no numbers), manual per-episode matching saved for future episodes.
- Audio tab: delay, default track/language, equalizer (libVLC equalizer) with friendly presets, audio filters.
- Video tab: brightness/contrast/saturation/hue/gamma, rotate/flip, zoom, aspect ratio, with live preview thumbnails (GPU/shader or CPU on a sampled frame).
- Playback tab: speed, sleep timer, toggles (show subtitle timeline, waveform seek bar, metadata).
- PRESETS: save/load/rename/delete named presets (subtitle delay+style+default track, audio delay+default track+EQ, video filters), auto-apply to a show/folder, persisted in SQLite.
```

**القبول:** حفظ preset لمسلسل وتطبيقه تلقائياً على الحلقة التالية.

---

## المرحلة 7 — ميزات متقدمة

```
Phase 7 — Advanced features.
- Bookmarks (Ctrl+B quick bookmark, sidebar timeline list, rename/delete, markers on seek bar) + chapter list.
- Waveform seek bar (decode audio separately with `symphonia`, cache, downsample).
- Metadata panel.
- OpenSubtitles integration (REST API, user-supplied API key/login in settings, search by hash/name, download + auto-match, rate-limit handling, graceful offline).
- Continue-watching prompt placed near the center playback controls with a short countdown.
- Music mode: large cover art, waveform bar, albums/artists views.
```

---

## المرحلة 8 — الإعدادات والـ Onboarding

```
Phase 8 — Settings, hotkeys, onboarding.
- Settings window with search; categories: General (theme light/dark/system, accent color), Interface, Privacy, Controls, Playback, Video, Audio, Subtitles (+OSD), Library, Network, Performance, Recording/Streaming, Extensions. Basic settings first, "Advanced" section at the bottom of each category. Every setting has an info icon with help text. No numeric-coded options; no duplicate settings.
- Hotkeys dialog (own dialog, remap, conflict detection, reset, import/export).
- Onboarding (theme + accent + shortcut intro), shown once.
- Full Arabic translation pass and RTL QA for every screen.
```

---

## المرحلة 9 — نوافذ الفتح والتحويل والبث

```
Phase 9 — Media dialogs, each its own simple dialog: Open file, Open folder, Network stream, Capture device (DirectShow via libVLC), Open from clipboard, Recent media, Convert (libVLC transcode via sout with presets, progress, cancel), Stream to another device (sout/HTTP/RTSP presets). Reuse the design system; proper error messages.
```

---

## المرحلة 10 — محرر التخطيط والـ Profiles

```
Phase 10 — Layout customization. Customize screen with a central frame showing the control bar; drag & drop to add/remove/reorder controls from a palette; snap guides and outlines; live preview; reset to default. Layout profiles: each profile stores layout + selected settings; switch/rename/duplicate/delete/export/import (JSON).
```

---

## المرحلة 11 — الجودة والتغليف

```
Phase 11 — Hardening & release.
- Performance: startup time, memory, 10k+ library, seek latency, 4K playback, long sessions (leak check).
- Robustness: missing/corrupt libvlc, unsupported files, network failures, permission errors; crash logging + opt-in diagnostics file.
- Accessibility: keyboard navigation, focus rings, high contrast, screen-reader names.
- Full Figma comparison of every screen at 100/125/150/200% DPI, light + dark, EN + AR.
- Packaging: portable zip + installer (WiX or Inno Setup) bundling libvlc.dll, libvlccore.dll, plugins/, app icon, THIRD_PARTY_LICENSES (LGPL notices), file associations, uninstaller.
- GitHub Actions: build, test, package artifacts. Final README and docs/testing.md complete.
```

---

## كيف تشغّلها

1. ضع `CLAUDE.md` وصور Figma و`tokens.md` و`SPEC.md` في المشروع.
2. أرسل مرحلة واحدة في كل جلسة، وشغّل التطبيق بنفسك وقارنه بـ Figma قبل المتابعة.
3. إن ظهر خطأ، الصق الرسالة للوكيل ثم هنا إن احتجت مساعدة.
4. أصعب نقطتين: المرحلة 5 (رسم الترجمة بنفسك) والمرحلة 6 (معاينات الفلاتر). خصّص لهما وقتاً أطول.
