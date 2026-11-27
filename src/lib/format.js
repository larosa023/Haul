// Formatting helpers (plain JS, unit-tested with node).

export function bytes(n) {
  if (!n || n < 0) return '0 B';
  const u = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${u[i]}`;
}

let bits = false;
/** Show speeds in bits per second (Mbps, like internet plans) instead of bytes (MB/s). */
export function setSpeedBits(on) {
  bits = !!on;
}

export function speed(n) {
  if (!n) return '';
  if (bits) {
    const b = n * 8;
    if (b >= 1e9) return `${(b / 1e9).toFixed(2)} Gbps`;
    if (b >= 1e6) return `${b >= 1e8 ? Math.round(b / 1e6) : (b / 1e6).toFixed(1)} Mbps`;
    return `${Math.max(1, Math.round(b / 1e3))} Kbps`;
  }
  return `${bytes(n)}/s`;
}

export function eta(secs) {
  if (secs == null || secs < 0 || !isFinite(secs)) return '';
  if (secs < 60) return `${Math.max(1, Math.round(secs))}s`;
  const m = Math.floor(secs / 60);
  if (m < 60) return `${m}m ${Math.round(secs % 60)}s`;
  const h = Math.floor(m / 60);
  if (h < 48) return `${h}h ${m % 60}m`;
  return `${Math.floor(h / 24)}d ${h % 24}h`;
}

export function duration(secs) {
  if (!secs) return '0m';
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return h ? `${h}h ${m}m` : `${m}m`;
}

export function percent(done, size) {
  if (!size) return 0;
  return Math.max(0, Math.min(100, (done / size) * 100));
}

export function date(unix) {
  if (!unix) return '';
  return new Date(unix * 1000).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
}

/** Status shown in the list. */
export function statusLabel(item) {
  switch (item.status) {
    case 'queued': return 'Queued';
    case 'metadata': return 'Getting info';
    case 'checking': return 'Checking';
    case 'downloading': return 'Downloading';
    case 'seeding': return 'Seeding';
    case 'paused': return 'Paused';
    case 'completed': return 'Completed';
    case 'error': return 'Error';
    default: return item.status;
  }
}

/** Which sidebar filter an item belongs to. */
export function matchesFilter(item, filter) {
  switch (filter) {
    case 'downloading': return ['queued', 'metadata', 'checking', 'downloading'].includes(item.status);
    case 'seeding': return item.status === 'seeding';
    case 'completed': return item.status === 'completed' || item.status === 'seeding';
    case 'paused': return item.status === 'paused';
    case 'errors': return item.status === 'error';
    case 'torrents': return item.kind === 'torrent';
    case 'direct': return item.kind === 'http';
    case 'media': return item.kind === 'media';
    default: return true;
  }
}

export function matchesSearch(item, q) {
  if (!q) return true;
  const hay = `${item.name} ${item.category} ${item.source ?? ''} ${item.fileName ?? ''}`.toLowerCase();
  return q.toLowerCase().split(/\s+/).filter(Boolean).every((w) => hay.includes(w));
}

/** Is this text a link Haul can download? */
export function linkKind(text) {
  const t = String(text ?? '').trim();
  if (/^magnet:\?/i.test(t)) return 'torrent';
  // A .torrent file on disk (opened from Explorer).
  if (/^([a-z]:\\|\\\\)/i.test(t) && /\.torrent$/i.test(t)) return 'torrent';
  try {
    const u = new URL(t);
    if (/^ftp(e?s)?:$/.test(u.protocol)) return 'http';
    if (u.protocol !== 'http:' && u.protocol !== 'https:') return null;
    if (/\.torrent$/i.test(u.pathname)) return 'torrent';
    if (/\.(m3u8|mpd)$/i.test(u.pathname) || isMediaSite(t)) return 'media';
    return 'http';
  } catch {
    return null;
  }
}

/** File name a URL points at. */
export function urlName(text) {
  try {
    const u = new URL(String(text).trim());
    if (u.protocol === 'magnet:') {
      return u.searchParams.get('dn') ?? 'Magnet link';
    }
    const seg = u.pathname.split('/').filter(Boolean).pop();
    return seg ? decodeURIComponent(seg) : u.hostname;
  } catch {
    const m = /[?&]dn=([^&]+)/.exec(String(text));
    return m ? decodeURIComponent(m[1].replace(/\+/g, ' ')) : String(text);
  }
}

const MEDIA_HOSTS = /(^|\.)(youtube\.com|youtu\.be|music\.youtube\.com|vimeo\.com|dailymotion\.com|twitch\.tv|tiktok\.com|twitter\.com|x\.com|instagram\.com|facebook\.com|fb\.watch|reddit\.com|v\.redd\.it|soundcloud\.com|bandcamp\.com|rumble\.com|bilibili\.com|nicovideo\.jp|streamable\.com|odysee\.com|mixcloud\.com|kick\.com|bsky\.app|threads\.net|ted\.com|archive\.org|bitchute\.com|vk\.com|9gag\.com|imgur\.com|pinterest\.com|tumblr\.com|loom\.com|wistia\.com|peertube\.[a-z]+)$/i;

/** A page on a site yt-dlp knows (a video, playlist or channel, not the site's home page). */
export function isMediaSite(text) {
  try {
    const u = new URL(String(text).trim());
    if (!/^https?:$/.test(u.protocol) || !MEDIA_HOSTS.test(u.hostname.replace(/^www\.|^m\./, ''))) return false;
    return u.pathname.length > 1 || u.search.length > 1;
  } catch {
    return false;
  }
}

/**
 * Expands a batch pattern: "file[001-100].zip" (keeps zero padding), "img[1-20:2].jpg" (step),
 * "part[a-f].rar". Several ranges multiply. Returns null if there's no pattern.
 */
export function expandPattern(text, cap = 5000) {
  const s = String(text ?? '').trim();
  const re = /\[(\d+)-(\d+)(?::(\d+))?\]|\[([a-zA-Z])-([a-zA-Z])\]/g;
  const parts = [];
  let last = 0;
  let m;
  while ((m = re.exec(s))) {
    parts.push(s.slice(last, m.index));
    if (m[1] != null) {
      const a = parseInt(m[1], 10);
      const b = parseInt(m[2], 10);
      const step = Math.max(1, parseInt(m[3] ?? '1', 10));
      const width = m[1].length > 1 && m[1].startsWith('0') ? m[1].length : 0;
      const vals = [];
      for (let i = a; a <= b ? i <= b : i >= b; i += a <= b ? step : -step) {
        vals.push(width ? String(i).padStart(width, '0') : String(i));
        if (vals.length > cap) break;
      }
      parts.push(vals);
    } else {
      const a = m[4].charCodeAt(0);
      const b = m[5].charCodeAt(0);
      const vals = [];
      for (let i = a; a <= b ? i <= b : i >= b; i += a <= b ? 1 : -1) vals.push(String.fromCharCode(i));
      parts.push(vals);
    }
    last = re.lastIndex;
  }
  if (!parts.length) return null;
  parts.push(s.slice(last));
  let out = [''];
  for (const p of parts) {
    if (typeof p === 'string') out = out.map((x) => x + p);
    else {
      const next = [];
      for (const x of out) for (const v of p) {
        next.push(x + v);
        if (next.length >= cap) break;
      }
      out = next;
    }
    if (out.length >= cap) out = out.slice(0, cap);
  }
  return out;
}

/** Links in pasted text: one per line (or space separated), patterns expanded. */
export function batchLinks(text, cap = 5000) {
  const out = [];
  const seen = new Set();
  // One per line; a line with several links separated by spaces works too, but a lone file path
  // with spaces in it (C:\My files\a.torrent) stays whole.
  const toks = [];
  for (const line of String(text ?? '').split(/\r?\n/)) {
    const l = line.trim();
    if (!l) continue;
    if (linkKind(l) || !/\s/.test(l)) toks.push(l);
    else toks.push(...l.split(/\s+/));
  }
  for (const tok of toks) {
    for (const u of expandPattern(tok, cap) ?? [tok]) {
      if (linkKind(u) && !seen.has(u)) {
        seen.add(u);
        out.push(u);
      }
      if (out.length >= cap) return out;
    }
  }
  return out;
}

export function clock(secs) {
  if (!secs || secs < 0) return '';
  const s = Math.round(secs);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
}
