// Haul browser extension: hands downloads to Haul (http://127.0.0.1:<port>), falling back to the
// browser's own downloader whenever Haul isn't running.

const DEFAULTS = { enabled: true, port: 17771, minSizeKb: 0, skipTypes: '' };
let bypassUntil = 0;

async function settings() {
  return { ...DEFAULTS, ...(await chrome.storage.sync.get(DEFAULTS)) };
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
    const list = await chrome.cookies.getAll({ url });
    return list.map((c) => `${c.name}=${c.value}`).join('; ');
  } catch {
    return '';
  }
}

async function send(url, { filename = '', referrer = '', fileSize = 0 } = {}) {
  const s = await settings();
  const res = await haul('/add', { url, filename, referrer, cookies: await cookiesFor(url), userAgent: navigator.userAgent, fileSize }, s.port);
  return !!res?.ok;
}

function sendable(url) {
  return /^(https?:|magnet:)/i.test(url || '');
}

// Downloads the browser starts go to Haul instead (unless Alt was held, or Haul is closed).
chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
  (async () => {
    const s = await settings();
    const url = item.finalUrl || item.url;
    const name = (item.filename || '').split(/[\\/]/).pop();
    const ext = (name.split('.').pop() || '').toLowerCase();
    const skipped = s.skipTypes.split(/[\s,]+/).filter(Boolean).map((x) => x.replace(/^\./, '').toLowerCase());
    const tooSmall = s.minSizeKb > 0 && item.fileSize > 0 && item.fileSize < s.minSizeKb * 1024;
    if (!s.enabled || Date.now() < bypassUntil || !sendable(url) || tooSmall || skipped.includes(ext) || item.byExtensionId === chrome.runtime.id) {
      suggest();
      return;
    }
    const ping = await haul('/ping', null, s.port);
    if (!ping) {
      suggest(); // Haul isn't running: the browser keeps the download.
      return;
    }
    chrome.downloads.cancel(item.id, () => {
      chrome.downloads.erase({ id: item.id });
    });
    const ok = await send(url, { filename: name, referrer: item.referrer || '', fileSize: item.fileSize > 0 ? item.fileSize : 0 });
    if (!ok) {
      // Haul refused it: download it in the browser after all.
      bypassUntil = Date.now() + 5000;
      chrome.downloads.download({ url, filename: name || undefined });
    }
  })();
  return true; // we call suggest() asynchronously
});

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({ id: 'haul-link', title: 'Download with Haul', contexts: ['link'] });
  chrome.contextMenus.create({ id: 'haul-media', title: 'Download with Haul', contexts: ['video', 'audio', 'image'] });
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  const url = info.menuItemId === 'haul-media' ? info.srcUrl : info.linkUrl;
  if (!sendable(url)) return;
  const ok = await send(url, { referrer: tab?.url || info.pageUrl || '' });
  if (!ok) {
    chrome.action.setBadgeBackgroundColor({ color: '#d64545' });
    chrome.action.setBadgeText({ text: '!' });
    setTimeout(() => chrome.action.setBadgeText({ text: '' }), 4000);
  }
});

chrome.runtime.onMessage.addListener((msg, _sender, reply) => {
  if (msg?.type === 'bypass') {
    bypassUntil = Date.now() + 4000;
  } else if (msg?.type === 'status') {
    settings().then((s) => haul('/ping', null, s.port).then((p) => reply({ running: !!p, version: p?.version ?? '' })));
    return true;
  }
});
