import { SvelteSet } from 'svelte/reactivity';
import { api, on, type AddRequest, type Item, type Settings, type Totals, type TorrentMeta, type UpdateState } from './api';
import { matchesFilter, matchesSearch } from './format.js';

export type Filter = 'all' | 'downloading' | 'seeding' | 'completed' | 'paused' | 'errors';

type Dialog =
  | { kind: 'add'; url?: string; request?: AddRequest }
  | { kind: 'torrent'; meta: TorrentMeta }
  | { kind: 'remove'; ids: number[] }
  | { kind: 'limits'; ids: number[] }
  | null;

export const app = $state({
  ready: false,
  items: {} as Record<number, Item>,
  order: [] as number[],
  settings: null as Settings | null,
  totals: null as Totals | null,
  update: null as UpdateState | null,
  version: '',
  portable: false,
  extensionDir: '',
  dataDir: '',
});

export const ui = $state({
  view: 'list' as 'list' | 'settings',
  settingsTab: 'general',
  filter: 'all' as Filter,
  search: '',
  anchor: 0,
  focus: 0,
  details: true,
  detailsTab: 'general',
  dialog: null as Dialog,
  clipboard: '' as string,
  toasts: [] as { id: number; text: string; kind: string; action?: { label: string; run: () => void } }[],
  updateDismissed: false,
});

export const selection = new SvelteSet<number>();

let toastId = 0;
export function toast(text: string, kind: 'info' | 'success' | 'error' = 'info', ms = 4500, action?: { label: string; run: () => void }) {
  const id = ++toastId;
  ui.toasts = [...ui.toasts, { id, text, kind, action }];
  setTimeout(() => dismissToast(id), ms);
}

export function dismissToast(id: number) {
  ui.toasts = ui.toasts.filter((t) => t.id !== id);
}

export function errorText(e: unknown): string {
  return String((e as Error)?.message ?? e).replace(/^Error:\s*/, '');
}

/** Items in queue order, filtered and searched. */
export function visibleItems(): Item[] {
  const out: Item[] = [];
  for (const id of app.order) {
    const it = app.items[id];
    if (it && matchesFilter(it, ui.filter) && matchesSearch(it, ui.search)) out.push(it);
  }
  return out;
}

export function counts(): Record<Filter, number> {
  const c: Record<Filter, number> = { all: 0, downloading: 0, seeding: 0, completed: 0, paused: 0, errors: 0 };
  for (const id of app.order) {
    const it = app.items[id];
    if (!it) continue;
    c.all++;
    for (const f of ['downloading', 'seeding', 'completed', 'paused', 'errors'] as Filter[]) if (matchesFilter(it, f)) c[f]++;
  }
  return c;
}

export function selectedItems(): Item[] {
  return [...selection].map((id) => app.items[id]).filter(Boolean);
}

export function selectOnly(id: number) {
  selection.clear();
  selection.add(id);
  ui.anchor = id;
  ui.focus = id;
}

export async function saveSettings(patch: Partial<Settings>) {
  if (!app.settings) return;
  const next = { ...$state.snapshot(app.settings), ...patch } as Settings;
  app.settings = next;
  try {
    app.settings = await api.setSettings(next);
  } catch (e) {
    toast(errorText(e), 'error', 8000);
  }
}

export async function load() {
  const s = await api.state();
  const items: Record<number, Item> = {};
  for (const it of s.items) items[it.id] = it;
  app.items = items;
  app.order = s.items.map((i) => i.id);
  app.settings = s.settings;
  app.totals = s.totals;
  app.update = s.update;
  app.version = s.version;
  app.portable = s.portable;
  app.extensionDir = s.extensionDir;
  app.dataDir = s.dataDir;
  app.ready = true;
}

export function listenAll() {
  on<Item[]>('items', (list) => {
    for (const it of list) {
      if (!app.items[it.id]) app.order.push(it.id);
      app.items[it.id] = it;
    }
  });
  on<number[]>('removed', (ids) => {
    for (const id of ids) {
      delete app.items[id];
      selection.delete(id);
    }
    app.order = app.order.filter((id) => app.items[id]);
  });
  on<Totals>('totals', (t) => (app.totals = t));
  on<UpdateState>('update', (u) => {
    app.update = u;
    if (u.status === 'ready') ui.updateDismissed = false;
  });
  on<Settings>('settings', (s) => (app.settings = s));
  on<{ text: string; kind: 'info' | 'success' | 'error' }>('toast', (t) => toast(t.text, t.kind, t.kind === 'error' ? 9000 : 4500));
  on<number>('select', (id) => {
    ui.view = 'list';
    ui.filter = 'all';
    selectOnly(id);
  });
  on<TorrentMeta>('torrent-meta', (meta) => {
    // One file picker at a time; others wait (see nextPending).
    if (ui.dialog) return;
    ui.view = 'list';
    ui.dialog = { kind: 'torrent', meta };
  });
  on<AddRequest>('add-request', (request) => {
    ui.view = 'list';
    ui.dialog = { kind: 'add', url: request.url, request };
  });
  on<{ url: string }>('clipboard-link', ({ url }) => {
    if (!ui.dialog) ui.clipboard = url;
  });
  on('open-add', () => {
    ui.view = 'list';
    ui.dialog = { kind: 'add' };
  });
}

/** Re-sync order after the backend moved things. */
export async function refreshOrder(order?: number[]) {
  app.order = order ?? (await api.order());
}

/** Opens the next torrent waiting for its files to be picked, if any. */
export async function nextPending() {
  if (ui.dialog) return;
  const list = await api.pendingTorrents().catch(() => []);
  if (list?.length && !ui.dialog) {
    ui.view = 'list';
    ui.dialog = { kind: 'torrent', meta: list[0] };
  }
}
