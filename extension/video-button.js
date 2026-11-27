// "Download video with Haul": a small button on pages with a video. Click it and Haul looks the
// page up with yt-dlp, so you can pick the quality (or audio only).
(() => {
  if (window.top !== window || window.__haulVideoButton) return;
  window.__haulVideoButton = true;
  const ext = globalThis.browser ?? globalThis.chrome;
  const WATCH = /(youtube\.com\/(watch|shorts\/|live\/)|youtu\.be\/|vimeo\.com\/\d|dailymotion\.com\/video|twitch\.tv\/(videos|[^/]+\/clip)|tiktok\.com\/@[^/]+\/video|(x|twitter)\.com\/[^/]+\/status|instagram\.com\/(reel|p)\/|reddit\.com\/r\/[^/]+\/comments|rumble\.com\/v|bilibili\.com\/video|soundcloud\.com\/[^/]+\/[^/]+)/i;
  let host = null;
  let dismissedFor = '';

  function big(v) {
    const r = v.getBoundingClientRect();
    return r.width >= 320 && r.height >= 180;
  }

  function make() {
    host = document.createElement('div');
    host.style.cssText = 'position:fixed;right:18px;bottom:18px;z-index:2147483647;';
    const root = host.attachShadow({ mode: 'closed' });
    root.innerHTML = `<style>
      .b{display:flex;align-items:center;gap:8px;font:600 13px 'Segoe UI Variable','Segoe UI',system-ui,sans-serif;color:#fff;background:#4f8cff;
         border:0;border-radius:20px;padding:9px 14px 9px 12px;box-shadow:0 6px 20px rgba(0,0,0,.35);cursor:pointer;opacity:.92;transition:opacity .15s,transform .15s}
      .b:hover{opacity:1;transform:translateY(-1px)} .x{margin-left:4px;opacity:.7;font-size:15px;line-height:1;padding:0 2px} .x:hover{opacity:1}
      svg{flex-shrink:0}</style>
      <button class="b" title="Send this video to Haul to pick the quality"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4v11M7.5 10.5 12 15l4.5-4.5M5 19h14"/></svg><span>Download video with Haul</span><span class="x" title="Hide">×</span></button>`;
    const btn = root.querySelector('.b');
    const label = root.querySelector('span');
    root.querySelector('.x').addEventListener('click', (e) => {
      e.stopPropagation();
      dismissedFor = location.href;
      host.remove();
    });
    btn.addEventListener('click', async () => {
      label.textContent = 'Sending…';
      let r = null;
      try {
        r = await ext.runtime.sendMessage({ type: 'media', url: location.href });
      } catch {}
      label.textContent = r?.ok ? 'Sent to Haul ✓' : 'Haul isn’t running';
      setTimeout(() => (label.textContent = 'Download video with Haul'), 2500);
    });
  }

  async function check() {
    let show = false;
    try {
      const s = await ext.storage.sync.get({ videoButton: true });
      show = s.videoButton && dismissedFor !== location.href && (WATCH.test(location.href) || [...document.querySelectorAll('video')].some(big));
    } catch {
      return;
    }
    if (show && !host?.isConnected) {
      if (!host) make();
      document.documentElement.appendChild(host);
    } else if (!show && host?.isConnected) {
      host.remove();
    }
  }

  // Video sites change pages without reloading; check now and then, cheaply.
  let pending = 0;
  const later = () => {
    if (!pending) pending = setTimeout(() => ((pending = 0), check()), 900);
  };
  new MutationObserver(later).observe(document.documentElement, { childList: true, subtree: true });
  addEventListener('popstate', later);
  check();
})();
