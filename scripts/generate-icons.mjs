// Generates the two Figma icon sets as plain SVG assets (no dependencies).
// mono: single-colour control icons. two-tone: menu icons with accent + secondary.
import fs from 'node:fs';
import path from 'node:path';

const root = path.resolve(process.argv[2] ?? '.');
fs.mkdirSync(path.join(root, 'mono'), { recursive: true });
fs.mkdirSync(path.join(root, 'two-tone'), { recursive: true });
const shapes = {
  play: [['M8 5v14l11-7z', 'fill']],
  pause: [['M6 5h4v14H6zM14 5h4v14h-4z', 'fill']],
  next: [['M6 5v14l9-7zM17 5h2v14h-2z', 'fill']],
  previous: [['M18 5v14l-9-7zM5 5h2v14H5z', 'fill']],
  stop: [['M6 6h12v12H6z', 'fill']],
  close: [['M6 6l12 12M18 6L6 18', 'stroke']],
  minimize: [['M5 12h14', 'stroke']],
  maximize: [['M5 5h14v14H5z', 'stroke']],
  fullscreen: [['M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5', 'stroke']],
  folder: [['M3 6h7l2 2h9v11H3z', 'fill']],
  home: [['M3 11l9-8 9 8v10h-6v-6H9v6H3z', 'fill']],
  videos: [['M3 5h18v14H3z', 'stroke'], ['M10 9l6 3-6 3z', 'fill']],
  songs: [['M9 17V6l10-2v11', 'stroke'], ['M7 18a2 2 0 1 0 4 0 2 2 0 0 0-4 0M17 16a2 2 0 1 0 4 0 2 2 0 0 0-4 0', 'fill']],
  subtitles: [['M3 5h18v14H3z', 'stroke'], ['M6 10h6M6 14h10', 'stroke']],
  audio: [['M4 10v4h4l5 4V6L8 10z', 'fill'], ['M16 9a4 4 0 0 1 0 6M18.5 6.5a7.5 7.5 0 0 1 0 11', 'stroke']],
  playlist: [['M4 6h11M4 11h11M4 16h7', 'stroke'], ['M16 14l5 3-5 3z', 'fill']],
  settings: [['M4 7h16M4 12h16M4 17h16', 'stroke'], ['M9 4v6M15 9v6M8 14v6', 'stroke']],
  search: [['M11 4a7 7 0 1 1 0 14 7 7 0 0 1 0-14z', 'stroke'], ['M16 16l5 5', 'stroke']],
  chevronDown: [['M6 9l6 6 6-6', 'stroke']],
  chevronUp: [['M6 15l6-6 6 6', 'stroke']],
  chevronLeft: [['M15 6l-6 6 6 6', 'stroke']],
  chevronRight: [['M9 6l6 6-6 6', 'stroke']],
  volume: [['M4 10v4h4l5 4V6L8 10z', 'fill'], ['M16 9a4 4 0 0 1 0 6', 'stroke']],
  queue: [['M4 6h16M4 11h16M4 16h9', 'stroke'], ['M17 15v6M14 18h6', 'stroke']],
  add: [['M12 5v14M5 12h14', 'stroke']],
  remove: [['M5 12h14', 'stroke']],
  check: [['M5 13l4 4L19 7', 'stroke']],
  more: [['M6 12h.01M12 12h.01M18 12h.01', 'stroke']],
  sidebar: [['M3 5h18v14H3z', 'stroke'], ['M9 5v14', 'stroke']],
  streaming: [['M12 4a8 8 0 1 1 0 16 8 8 0 0 1 0-16z', 'stroke'], ['M10 9l6 3-6 3z', 'fill']],
};

const ACCENT = '#D37918';
const SECONDARY = '#8F8F8F';
const svg = (body) => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24">${body}</svg>\n`;
const render = (parts, colors) =>
  parts.map(([d, mode], i) => {
    const color = colors[i % colors.length];
    return mode === 'fill'
      ? `<path d="${d}" fill="${color}"/>`
      : `<path d="${d}" fill="none" stroke="${color}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>`;
  }).join('');

for (const [name, parts] of Object.entries(shapes)) {
  fs.writeFileSync(path.join(root, 'mono', `${name}.svg`), svg(render(parts, ['#FFFFFF'])));
}
const twoTone = ['settings', 'playlist', 'subtitles', 'audio', 'home', 'videos', 'songs', 'folder', 'sidebar', 'streaming', 'queue', 'volume'];
for (const name of twoTone) {
  const parts = shapes[name];
  fs.writeFileSync(
    path.join(root, 'two-tone', `${name}.svg`),
    svg(render(parts, [ACCENT, SECONDARY])),
  );
}
console.log(`mono=${Object.keys(shapes).length} two-tone=${twoTone.length}`);
