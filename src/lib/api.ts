import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type Kind = 'http' | 'torrent';
export type Status = 'queued' | 'metadata' | 'checking' | 'downloading' | 'seeding' | 'paused' | 'completed' | 'error';

export interface Item {
  id: number;
  kind: Kind;
  name: string;
  source: string;
  saveDir: string;
  fileName: string;
  category: string;
  size: number;
  done: number;
  uploaded: number;
  added: number;
  finishedAt: number;
  priority: number;
  limitDown: number;
  limitUp: number;
  paused: boolean;
  status: Status;
  error: string;
  sha256: string;
  hashState: string;
  resumable: boolean;
  connections: number;
  segments: number;
  infoHash: string;
  sequential: boolean;
  ratioLimit: number;
  seedTimeLimit: number;
  seedingSecs: number;
  speedDown: number;
  speedUp: number;
  eta: number;
  peers: number;
  note: string;
  fileCount: number;
}

export interface Rule {
  enabled: boolean;
  category: string;
  extensions: string[];
  minSizeMb: number;
  maxSizeMb: number;
  site: string;
}

export interface Settings {
  closeToTray: boolean;
  startMinimized: boolean;
  launchAtStartup: boolean;
  watchClipboard: boolean;
  showAddDialog: boolean;
  notifyComplete: boolean;
  registerHandlers: boolean;
  downloadDir: string;
  connections: number;
  maxRetries: number;
  maxActive: number;
  userAgent: string;
  sortEnabled: boolean;
  folders: Record<string, string>;
  rules: Rule[];
  autoExtract: boolean;
  deleteArchiveAfterExtract: boolean;
  downLimit: number;
  upLimit: number;
  altDownLimit: number;
  altUpLimit: number;
  altEnabled: boolean;
  schedule: { enabled: boolean; from: string; to: string; days: boolean[] };
  listenPort: number;
  dht: boolean;
  upnp: boolean;
  natpmp: boolean;
  encryption: 'prefer' | 'require' | 'disabled';
  blocklistEnabled: boolean;
  blocklist: string;
  maxActiveSeeds: number;
  ratioLimit: number;
  seedTimeLimit: number;
  bindAdapter: string;
  preallocate: boolean;
  bridgeEnabled: boolean;
  bridgePort: number;
  theme: string;
  accent: string;
  nativeTitleBar: boolean;
  compactRows: boolean;
  autoUpdate: boolean;
  channel: 'stable' | 'beta';
  skipVersion: string;
  window: unknown;
  initialized: boolean;
}

export interface EngineStatus {
  running: boolean;
  error: string;
  port: number;
  dht: boolean;
  upnp: boolean;
  natpmp: string;
  blocklist: string;
  adapter: string;
  adapterDown: boolean;
  encryption: string;
}

export interface Totals {
  down: number;
  up: number;
  downloading: number;
  seeding: number;
  alt: boolean;
  engine: EngineStatus;
}

export interface UpdateState {
  status: 'idle' | 'checking' | 'none' | 'available' | 'downloading' | 'ready' | 'applying' | 'error';
  current: string;
  version: string;
  notes: string;
  pageUrl: string;
  progress: number;
  error: string;
  waiting: boolean;
  checkedAt: number;
}

export interface AppState {
  items: Item[];
  settings: Settings;
  totals: Totals;
  update: UpdateState;
  version: string;
  portable: boolean;
  dataDir: string;
  extensionDir: string;
}

export interface ProbeInfo {
  name: string;
  size: number;
  resumable: boolean;
  finalUrl: string;
  etag: string;
  lastModified: string;
  category: string;
  folder: string;
}

export interface TorrentMeta {
  id: number;
  name: string;
  hash: string;
  files: [string, number][];
  folder: string;
  category: string;
  size: number;
}

export interface TorrentFile {
  index: number;
  path: string;
  size: number;
  done: number;
  priority: number;
}

export interface Adapter {
  name: string;
  description: string;
  index: number;
  ipv4: string | null;
  up: boolean;
}

export interface AddRequest {
  url: string;
  filename: string;
  referrer: string;
  cookies: string;
  userAgent: string;
  fileSize: number;
}

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(cmd, args);
}

export function on<T>(event: string, fn: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => fn(e.payload));
}

export const api = {
  state: () => call<AppState>('get_state'),
  setSettings: (s: Settings) => call<Settings>('set_settings', { new: s }),
  resetRules: () => call<Settings>('reset_rules'),
  inspect: (url: string, referrer?: string, cookies?: string) => call<ProbeInfo>('inspect_url', { url, referrer, cookies }),
  addHttp: (add: Record<string, unknown>) => call<number>('add_http', { add }),
  addTorrent: (add: { source?: string; bytes?: number[]; direct?: boolean }) => call<{ id: number; existing: boolean }>('add_torrent', { add }),
  confirmTorrent: (confirm: { id: number; folder: string; priorities: number[]; paused: boolean; sequential: boolean; category: string }) =>
    call<void>('confirm_torrent', { confirm }),
  cancelTorrent: (id: number) => call<void>('cancel_torrent', { id }),
  pause: (ids: number[]) => call<void>('pause', { ids }),
  resume: (ids: number[]) => call<void>('resume', { ids }),
  remove: (ids: number[], deleteFiles: boolean) => call<void>('remove', { ids, deleteFiles }),
  retry: (ids: number[]) => call<void>('retry', { ids }),
  move: (ids: number[], dir: 'top' | 'up' | 'down' | 'bottom') => call<number[]>('move_items', { ids, dir }),
  order: () => call<number[]>('get_order'),
  setOptions: (ids: number[], options: Record<string, unknown>) => call<void>('set_options', { ids, options }),
  setFilePriorities: (id: number, priorities: number[]) => call<void>('set_file_priorities', { id, priorities }),
  files: (id: number) => call<TorrentFile[]>('get_files', { id }),
  item: (id: number) => call<Record<string, unknown> | null>('get_item', { id }),
  open: (id: number) => call<void>('open_item', { id }),
  reveal: (id: number) => call<void>('reveal', { id }),
  openFolder: (path: string) => call<void>('open_folder', { path }),
  adapters: () => call<Adapter[]>('adapters'),
  handlerState: () => call<{ magnet: boolean; torrent: boolean }>('handler_state'),
  makeDefault: () => call<void>('make_default'),
  checkUpdate: () => call<UpdateState>('check_update'),
  applyUpdate: (force: boolean) => call<void>('apply_update', { force }),
  listThemes: () => call<{ file: string; json: string }[]>('list_themes'),
  saveTheme: (name: string, json: string) => call<string>('save_theme', { name, json }),
  openThemesFolder: () => call<void>('open_themes_folder'),
  systemTheme: () => call<{ accent: string | null; light: boolean; backdrop: boolean }>('system_theme'),
  openExtensionFolder: () => call<void>('open_extension_folder'),
  openDataFolder: () => call<void>('open_data_folder'),
  restartTorrents: () => call<void>('restart_torrents'),
  quit: () => call<void>('quit_app'),
  uiReady: () => call<void>('ui_ready'),
  setBackdrop: (kind: string, dark: boolean) => call<void>('set_backdrop', { kind, dark }),
  setNativeFrame: (native: boolean) => call<void>('set_native_frame', { native }),
};
