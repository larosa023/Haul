import { api, isTauri, type Settings } from './api';
import { THEMES, cssVars, effectiveTheme, parseThemeJson, resolveAccent, type ThemeDef } from './themecore.js';

export const themes = $state({
  custom: [] as ThemeDef[],
  errors: [] as string[],
  systemAccent: null as string | null,
  systemLight: false,
  backdrop: false,
});

export function allThemes(): ThemeDef[] {
  return [...THEMES, ...themes.custom];
}

export function findTheme(id: string): ThemeDef {
  return allThemes().find((t) => t.id === id) ?? THEMES[0];
}

/** Reads theme files from Haul's and Hangar's themes folders. */
export async function reloadThemes() {
  if (!isTauri) return;
  const files = await api.listThemes().catch(() => []);
  const seen = new Set<string>();
  const custom: ThemeDef[] = [];
  const errors: string[] = [];
  for (const f of files) {
    const r = parseThemeJson(f.json, f.file);
    if (r.theme && !seen.has(r.theme.id)) {
      seen.add(r.theme.id);
      custom.push(r.theme);
    } else if (r.error) {
      errors.push(`${f.file.split(/[\\/]/).pop()}: ${r.error}`);
    }
  }
  custom.sort((a, b) => a.name.localeCompare(b.name));
  themes.custom = custom;
  themes.errors = errors;
}

export async function refreshSystem() {
  if (!isTauri) return;
  const s = await api.systemTheme().catch(() => null);
  if (s) {
    themes.systemAccent = s.accent;
    themes.systemLight = s.light;
    themes.backdrop = s.backdrop;
  }
}

let lastBackdrop = '';

/** Applies a theme live: CSS variables, light/dark, and the Windows backdrop. */
export function applyTheme(settings: Pick<Settings, 'theme' | 'accent' | 'compactRows'>) {
  const chosen = findTheme(settings.theme);
  const t = effectiveTheme(chosen, themes.systemLight);
  const accent = resolveAccent(t, settings.accent, themes.systemAccent);
  const backdropOn = isTauri && t.backdrop !== 'none' && themes.backdrop;
  const vars = cssVars(t, accent, backdropOn);
  const root = document.documentElement;
  for (const [k, v] of Object.entries(vars)) root.style.setProperty(k, v);
  root.dataset.mode = t.dark ? 'dark' : 'light';
  root.dataset.compact = String(!!settings.compactRows);
  const key = `${backdropOn ? t.backdrop : 'none'}:${t.dark}`;
  if (isTauri && key !== lastBackdrop) {
    lastBackdrop = key;
    void api.setBackdrop(backdropOn ? t.backdrop : 'none', t.dark).catch(() => {});
  }
}

