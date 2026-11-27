// Haul browser extension (Chrome, Edge, Drift and other Chromium browsers, and Firefox):
// hands downloads to Haul (http://127.0.0.1:<port>), falling back to the browser's own
// downloader whenever Haul isn't running.

const ext = globalThis.browser ?? globalThis.chrome;
const DEFAULTS = { enabled: true, port: 17771, minSizeKb: 0, skipTypes: '', videoButton: true };
let bypassUntil = 0;

async function settings() {
  return { ...DEFAULTS, ...(await ext.storage.sync.get(DEFAULTS)) };
}

async function haul(path, body, port) {
  const ctl = new AbortController();
  const t = setTimeout(() => ctl.abort(), 2500);
  try {
    const r = await fetch(`http://127.0.0.1:${port}${path}`, {
      method: body ? 'POST' : 'GET',
      headers: body ? { 'Content-Type': 'application/json', 'X-Haul': '1' } : { 'X-Haul': '1' },
      body: body ? JSON.stringify(body) : undefined,
      signal: ctl.signal,
    });
    return r.ok ? await r.json() : null;
  } catch {
    return null;
  } finally {
    clearTimeout(t);
  }
}

async function cookiesFor(url) {
  try {
    const list = await ext.cookies.getAll({ url });
    return list.map((c) => `${c.name}=${c.value}`).join('; ');
  } catch {
    return '';
  }
}

async function send(url, { filename = '', referrer = '', fileSize = 0, media = false } = {}) {
  const s = await settings();
  const cookies = /^https?:/i.test(url) ? await cookiesFor(url) : '';
  const res = await haul('/add', { url, filename, referrer, cookies, userAgent: navigator.userAgent, fileSize, media }, s.port);
  return !!res?.ok;
}

function sendable(url) {
  return /^(https?:|ftps?:|magnet:)/i.test(url || '');
}

/** Should this download go to Haul? */
async function wanted(item, s) {
  const url = item.finalUrl || item.url;
  const name = (item.filename || '').split(/[\\/]/).pop();
  const ext2 = (name.split('.').pop() || '').toLowerCase();
  const skipped = s.skipTypes.split(/[\s,]+/).filter(Boolean).map((x) => x.replace(/^\./, '').toLowerCase());
  const tooSmall = s.minSizeKb > 0 && item.fileSize > 0 && item.fileSize < s.minSizeKb * 1024;
  if (!s.enabled || Date.now() < bypassUntil || !sendable(url) || tooSmall || skipped.includes(ext2) || item.byExtensionId === ext.runtime.id) return null;
  return { url, name };
}

async function handOver(item, s, w) {
  const ok = await send(w.url, { filename: w.name, referrer: item.referrer || '', fileSize: item.fileSize > 0 ? item.fileSize : 0 });
  if (!ok) {
    // Haul refused it: download it in the browser after all.
    bypassUntil = Date.now() + 5000;
    ext.downloads.download({ url: w.url, filename: w.name || undefined });
  }
}

if (ext.downloads.onDeterminingFilename) {
  // Chromium: decide before the file is created.
  ext.downloads.onDeterminingFilename.addListener((item, suggest) => {
    (async () => {
      const s = await settings();
      const w = await wanted(item, s);
      if (!w || !(await haul('/ping', null, s.port))) {
        suggest(); // the browser keeps it (Haul isn't running, or it's excluded)
        return;
      }
      ext.downloads.cancel(item.id, () => ext.downloads.erase({ id: item.id }));
      await handOver(item, s, w);
    })();
    return true; // suggest() is called asynchronously
  });
} else {
  // Firefox: no onDeterminingFilename, so cancel right after the download starts.
  ext.downloads.onCreated.addListener(async (item) => {
    const s = await settings();
    const w = await wanted(item, s);
    if (!w || !(await haul('/ping', null, s.port))) return;
    try {
      await ext.downloads.cancel(item.id);
      await ext.downloads.erase({ id: item.id });
    } catch {}
    await handOver(item, s, w);
  });
}

function menus() {
  ext.contextMenus.removeAll(() => {
    ext.contextMenus.create({ id: 'haul-link', title: 'Download with Haul', contexts: ['link'] });
    ext.contextMenus.create({ id: 'haul-media', title: 'Download with Haul', contexts: ['image', 'audio'] });
    ext.contextMenus.create({ id: 'haul-video', title: 'Download video with Haul', contexts: ['page', 'video', 'frame'] });
  });
}
ext.runtime.onInstalled.addListener(menus);
ext.runtime.onStartup?.addListener(menus);

function flag(ok) {
  if (ok) return;
  ext.action.setBadgeBackgroundColor({ color: '#d64545' });
  ext.action.setBadgeText({ text: '!' });
  setTimeout(() => ext.action.setBadgeText({ text: '' }), 4000);
}

ext.contextMenus.onClicked.addListener(async (info, tab) => {
  const page = tab?.url || info.pageUrl || '';
  if (info.menuItemId === 'haul-video') {
    // Video elements are usually blob: streams, so hand Haul the page and let yt-dlp find it.
    const src = info.srcUrl && /^https?:/i.test(info.srcUrl) && !/\.(m3u8|mpd)(\?|$)/i.test(info.srcUrl) && info.mediaType === 'video' ? info.srcUrl : '';
    flag(await send(src || info.frameUrl || page, { referrer: page, media: !src }));
    return;
  }
  const url = info.menuItemId === 'haul-media' ? info.srcUrl : info.linkUrl;
  if (!sendable(url)) return;
  flag(await send(url, { referrer: page }));
});

ext.runtime.onMessage.addListener((msg, sender, reply) => {
  if (msg?.type === 'bypass') {
    bypassUntil = Date.now() + 4000;
  } else if (msg?.type === 'status') {
    settings().then((s) => haul('/ping', null, s.port).then((p) => reply({ running: !!p, version: p?.version ?? '' })));
    return true;
  } else if (msg?.type === 'media') {
    send(msg.url, { referrer: sender?.tab?.url || '', media: true }).then((ok) => reply({ ok }));
    return true;
  }
});
