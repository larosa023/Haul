// The suite's 14 themes and accent rules, matching Hangar (src/themes.cpp) and docs/THEMES.md.
// Plain JS so it can be unit-tested with node.

/** @typedef {{ id: string, name: string, description: string, dark: boolean, backdrop: 'mica'|'acrylic'|'none', transparency: number, surfaceOpacity: number, followWindows?: boolean, custom?: boolean, file?: string,
 *   colors: { background: string, sidebar: string, surface: string, surfaceHover: string, surfaceStrong: string, text: string, subtext: string, faint: string, accent: string } }} ThemeDef */

/** @returns {ThemeDef} */
function def(id, name, description, dark, backdrop, transparency, bg, sidebar, surface, hover, strong, text, subtext, faint, accent) {
  return {
    id, name, description, dark, backdrop, transparency, surfaceOpacity: 1,
    colors: { background: bg, sidebar, surface, surfaceHover: hover, surfaceStrong: strong, text, subtext, faint, accent },
  };
}

/** @type {ThemeDef[]} */
export const THEMES = [
  def('dark', 'Dark', 'The default', true, 'mica', 0.2, '#16161a', '#111114', '#222228', '#2a2a31', '#34343c', '#f2f2f5', '#a8a8b3', '#6e6e7a', 'windows'),
  def('light', 'Light', 'Bright and clean', false, 'mica', 0.2, '#f5f5f7', '#ebebef', '#ffffff', '#f7f7fa', '#e6e6ec', '#16161a', '#5c5c68', '#8e8e9a', 'windows'),
  { ...def('system', 'Windows', 'Light or dark, like Windows', true, 'mica', 0.2, '#16161a', '#111114', '#222228', '#2a2a31', '#34343c', '#f2f2f5', '#a8a8b3', '#6e6e7a', 'windows'), followWindows: true },
  def('midnight', 'Midnight', 'Pure black for OLED screens', true, 'none', 0, '#000000', '#000000', '#0e0e10', '#16161a', '#202025', '#f5f5f7', '#a0a0aa', '#64646e', '#7aa2ff'),
  { ...def('glass', 'Glass', 'See-through, over your wallpaper', true, 'acrylic', 0.55, '#141418', '#0e0e12', '#ffffff', '#ffffff', '#ffffff', '#f7f7fa', '#c2c2cc', '#8a8a96', 'windows'), surfaceOpacity: 0.07 },
  def('frost', 'Frost', 'Cool arctic blues', true, 'mica', 0.12, '#2e3440', '#272c36', '#3b4252', '#434c5e', '#4c566a', '#eceff4', '#c0c8d6', '#7b8698', '#88c0d0'),
  def('twilight', 'Twilight', 'Deep violet', true, 'mica', 0.12, '#1e1b2e', '#19162a', '#2a2640', '#332e4d', '#3e3859', '#eeeaf7', '#b4adc9', '#7d7596', '#b794f6'),
  def('forest', 'Forest', 'Evergreen and calm', true, 'mica', 0.12, '#141b17', '#101612', '#1d2721', '#24302a', '#2e3b33', '#e8f0ea', '#a7b8ad', '#6c7f72', '#5fd08b'),
  def('sunset', 'Sunset', 'Warm and cozy', true, 'mica', 0.12, '#1c1614', '#17110f', '#2a201c', '#332722', '#3e302a', '#f6eee9', '#c4b3a9', '#8b776c', '#ff8a4c'),
  def('ocean', 'Ocean', 'Deep teal', true, 'mica', 0.12, '#0f1a20', '#0b151a', '#16252d', '#1c2e37', '#243843', '#e6f1f5', '#a3bac4', '#66808c', '#3cc8c8'),
  def('rose', 'Rose', 'Soft pink on plum', true, 'mica', 0.12, '#1d1519', '#181115', '#2a1f25', '#33262d', '#3e2e37', '#f7ecf1', '#c7afba', '#8d7380', '#ff7aa8'),
  def('graphite', 'Graphite', 'Monochrome, no color', true, 'none', 0, '#1a1a1a', '#141414', '#262626', '#2e2e2e', '#3a3a3a', '#f0f0f0', '#ababab', '#707070', '#e6e6e6'),
  def('paper', 'Paper', 'Warm light, easy on the eyes', false, 'none', 0, '#f4efe6', '#eae3d6', '#fbf8f2', '#f6f1e7', '#e5dccb', '#2b241b', '#6b5f50', '#9a8e7e', '#b5651d'),
  def('mist', 'Mist', 'Cool light gray-blue', false, 'mica', 0.15, '#eef2f6', '#e3e9ef', '#fafcfe', '#f3f7fa', '#dce4ec', '#17202a', '#56636f', '#8a97a3', '#2f7dd1'),
];

/** The suite's 9 accent swatches. */
export const ACCENT_SWATCHES = ['#4f8cff', '#9b6cf2', '#e86a92', '#ff5d5d', '#ff8a4c', '#f2b84b', '#3fb98e', '#2ec4c4', '#a0a0aa'];

export function parseHex(s) {
  if (typeof s !== 'string') return null;
  let h = s.trim().replace(/^#/, '');
  if (h.length === 3) h = h.split('').map((c) => c + c).join('');
  if (!/^[0-9a-fA-F]{6}$/.test(h)) return null;
  return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
}

export function toHex(rgb) {
  return '#' + rgb.map((v) => Math.round(Math.max(0, Math.min(255, v))).toString(16).padStart(2, '0')).join('');
}

/** Relative luminance-ish (as Hangar's Luma: perceived brightness 0..1). */
export function luma(hex) {
  const c = parseHex(hex);
  if (!c) return 0;
  return (0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]) / 255;
}

export function mix(a, b, t) {
  const x = parseHex(a), y = parseHex(b);
  if (!x || !y) return a;
  return toHex(x.map((v, i) => v + (y[i] - v) * t));
}

/**
 * Parses a theme file in the shared Hangar format. Missing colors come from Dark or Light.
 * @returns {{ theme?: ThemeDef, error?: string }}
 */
export function parseThemeJson(text, file = '') {
  let v;
  try {
    // Hangar's docs show // comments in examples; tolerate them.
    v = JSON.parse(String(text).replace(/^\s*\/\/.*$/gm, '').replace(/,\s*\/\/[^\n]*$/gm, ','));
  } catch {
    return { error: 'not valid JSON' };
  }
  if (!v || typeof v !== 'object') return { error: 'not valid JSON' };
  const name = String(v.name ?? '').trim();
  if (!name) return { error: 'missing "name"' };
  const dark = String(v.mode ?? 'dark').toLowerCase() !== 'light';
  let backdrop = String(v.backdrop ?? 'none').toLowerCase();
  if (backdrop !== 'mica' && backdrop !== 'acrylic') backdrop = 'none';
  const clamp = (x, lo, hi, d) => (typeof x === 'number' && isFinite(x) ? Math.min(hi, Math.max(lo, x)) : d);
  const base = THEMES[dark ? 0 : 1];
  const colors = { ...base.colors };
  const c = v.colors && typeof v.colors === 'object' ? v.colors : {};
  for (const key of ['background', 'sidebar', 'surface', 'surfaceHover', 'surfaceStrong', 'text', 'subtext', 'faint']) {
    if (c[key] == null || c[key] === '') continue;
    const rgb = parseHex(c[key]);
    if (!rgb) return { error: `bad color for ${key}` };
    colors[key] = toHex(rgb);
  }
  const accent = String(c.accent ?? 'windows').toLowerCase();
  if (accent === 'windows') colors.accent = 'windows';
  else {
    const rgb = parseHex(accent);
    if (!rgb) return { error: 'bad accent color' };
    colors.accent = toHex(rgb);
  }
  return {
    theme: {
      id: 'custom:' + String(v.id ?? name).toLowerCase(),
      name,
      description: String(v.description ?? 'Your theme'),
      dark,
      backdrop: /** @type {any} */ (backdrop),
      transparency: clamp(v.transparency, 0, 0.9, 0.15),
      surfaceOpacity: clamp(v.surfaceOpacity, 0.02, 1, 1),
      colors,
      custom: true,
      file,
    },
  };
}

/** A theme as a shareable JSON file (Hangar format). */
export function themeToJson(t, accentHex) {
  const out = {
    schema: 1,
    name: t.custom ? t.name : `${t.name} (copy)`,
    description: t.description,
    mode: t.dark ? 'dark' : 'light',
    backdrop: t.backdrop,
    transparency: t.transparency,
    colors: { ...t.colors, accent: accentHex ?? t.colors.accent },
  };
  if (t.surfaceOpacity < 1) out.surfaceOpacity = t.surfaceOpacity;
  return JSON.stringify(out, null, 2);
}

/**
 * The accent actually used: the user's choice ("theme", "windows" or "#hex"), kept readable on
 * the background (dark accents lightened on dark themes, bright ones darkened on light themes).
 */
export function resolveAccent(theme, setting, windowsAccent) {
  let a;
  if (setting && setting.startsWith('#') && parseHex(setting)) a = setting;
  else if (setting === 'theme' && theme.colors.accent !== 'windows') a = theme.colors.accent;
  else a = windowsAccent && parseHex(windowsAccent) ? windowsAccent : '#4f8cff';
  a = toHex(parseHex(a));
  if (theme.dark && luma(a) < 0.3) a = mix(a, '#ffffff', 0.35);
  if (!theme.dark && luma(a) > 0.55) a = mix(a, '#000000', 0.3);
  return a;
}

/**
 * CSS variables for a theme. `backdropOn` = Windows will draw Mica/Acrylic behind the window.
 * @param {ThemeDef} t
 */
export function cssVars(t, accent, backdropOn) {
  const tr = backdropOn ? t.transparency : 0;
  const alpha = (hex, a) => {
    const c = parseHex(hex);
    return a >= 0.999 ? hex : `rgb(${c[0]} ${c[1]} ${c[2]} / ${a.toFixed(3)})`;
  };
  const bg = t.colors.background;
  let surface = t.colors.surface, hover = t.colors.surfaceHover, strong = t.colors.surfaceStrong;
  if (t.surfaceOpacity < 1) {
    const o = t.surfaceOpacity;
    const layer = (c, k) => (backdropOn ? alpha(c, Math.min(1, o * k)) : mix(bg, c, Math.min(1, o * k)));
    surface = layer(surface, 1);
    hover = layer(hover, 1.6);
    strong = layer(strong, 2.3);
  }
  const accentHover = t.dark ? mix(accent, '#ffffff', 0.12) : mix(accent, '#000000', 0.1);
  return {
    '--bg': alpha(bg, 1 - tr),
    '--sidebar': alpha(t.colors.sidebar, backdropOn ? Math.max(0.25, 1 - tr * 2.2) : 1),
    '--surface': surface,
    '--surface-hover': hover,
    '--surface-strong': strong,
    '--line': t.dark ? 'rgb(255 255 255 / 0.08)' : 'rgb(0 0 0 / 0.09)',
    '--text': t.colors.text,
    '--text-2': t.colors.subtext,
    '--text-3': t.colors.faint,
    '--accent': accent,
    '--accent-hover': accentHover,
    '--on-accent': luma(accent) > 0.55 ? '#101014' : '#ffffff',
    '--ok': t.dark ? '#5bd18b' : '#1f9d57',
    '--warn': t.dark ? '#f2b84b' : '#b7791f',
    '--danger': t.dark ? '#ff6b6b' : '#d64545',
    '--info': t.dark ? '#6cb4ff' : '#2f6fdb',
    '--solid-bg': bg,
  };
}

/** The theme to draw: "Windows" follows the system light/dark setting. */
export function effectiveTheme(t, systemLight) {
  if (!t.followWindows) return t;
  return THEMES[systemLight ? 1 : 0];
}
