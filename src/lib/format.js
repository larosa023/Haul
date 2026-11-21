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

export function speed(n) {
  if (!n) return '';
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
  try {
    const u = new URL(t);
    if (u.protocol !== 'http:' && u.protocol !== 'https:') return null;
    return /\.torrent$/i.test(u.pathname) ? 'torrent' : 'http';
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
