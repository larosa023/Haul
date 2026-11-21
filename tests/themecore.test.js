import { test } from 'node:test';
import assert from 'node:assert/strict';
import { THEMES, ACCENT_SWATCHES, parseThemeJson, resolveAccent, cssVars, effectiveTheme, themeToJson, luma } from '../src/lib/themecore.js';

test('the 14 suite themes, in order, with Dark first', () => {
  assert.deepEqual(THEMES.map((t) => t.name), ['Dark', 'Light', 'Windows', 'Midnight', 'Glass', 'Frost', 'Twilight', 'Forest', 'Sunset', 'Ocean', 'Rose', 'Graphite', 'Paper', 'Mist']);
  assert.equal(THEMES[0].backdrop, 'mica');
  assert.equal(ACCENT_SWATCHES.length, 9);
});

test('parses the Hangar theme format, filling gaps from Dark/Light', () => {
  const { theme } = parseThemeJson(`{
    "schema": 1, "name": "Nordish", "mode": "light", "backdrop": "acrylic",
    "transparency": 2, "colors": { "background": "#abc", "accent": "#88C0D0" }
  }`);
  assert.equal(theme.name, 'Nordish');
  assert.equal(theme.dark, false);
  assert.equal(theme.backdrop, 'acrylic');
  assert.equal(theme.transparency, 0.9);
  assert.equal(theme.colors.background, '#aabbcc');
  assert.equal(theme.colors.text, THEMES[1].colors.text);
  assert.equal(theme.colors.accent, '#88c0d0');
  assert.equal(parseThemeJson('{}').error, 'missing "name"');
  assert.equal(parseThemeJson('{"name":"x","colors":{"text":"blue"}}').error, 'bad color for text');
  assert.equal(parseThemeJson('nope').error, 'not valid JSON');
});

test('round-trips through themeToJson', () => {
  const json = themeToJson(THEMES[5], '#88c0d0');
  const { theme } = parseThemeJson(json);
  assert.equal(theme.name, 'Frost (copy)');
  assert.equal(theme.colors.surface, THEMES[5].colors.surface);
});

test('accents stay readable', () => {
  const dark = THEMES[0], light = THEMES[1];
  assert.equal(resolveAccent(THEMES[3], 'theme', '#123456'), '#7aa2ff');
  assert.equal(resolveAccent(dark, 'theme', '#3c8ce6'), '#3c8ce6');
  assert.ok(luma(resolveAccent(dark, '#101010', null)) >= 0.25);
  assert.ok(luma(resolveAccent(light, '#ffff00', null)) < luma('#ffff00'));
  assert.equal(resolveAccent(dark, 'windows', null), '#4f8cff');
});

test('css variables and the Windows theme', () => {
  const v = cssVars(THEMES[4], '#4f8cff', true);
  assert.match(v['--surface'], /rgb\(255 255 255 \/ 0.070\)/);
  assert.match(v['--bg'], /0\.450/);
  const solid = cssVars(THEMES[4], '#4f8cff', false);
  assert.match(solid['--surface'], /^#/);
  assert.equal(effectiveTheme(THEMES[2], true).id, 'light');
  assert.equal(effectiveTheme(THEMES[2], false).id, 'dark');
});
