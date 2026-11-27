const ext = globalThis.browser ?? globalThis.chrome;
const DEFAULTS = { enabled: true, port: 17771, minSizeKb: 0, skipTypes: '', videoButton: true };
const $ = (id) => document.getElementById(id);

ext.storage.sync.get(DEFAULTS).then((s) => {
  $('enabled').checked = s.enabled;
  $('port').value = s.port;
  $('minSizeKb').value = s.minSizeKb;
  $('skipTypes').value = s.skipTypes;
  $('videoButton').checked = s.videoButton;
});

$('enabled').addEventListener('change', (e) => ext.storage.sync.set({ enabled: e.target.checked }));
$('videoButton').addEventListener('change', (e) => ext.storage.sync.set({ videoButton: e.target.checked }));
$('port').addEventListener('change', (e) => ext.storage.sync.set({ port: Number(e.target.value) || 17771 }).then(status));
$('minSizeKb').addEventListener('change', (e) => ext.storage.sync.set({ minSizeKb: Math.max(0, Number(e.target.value) || 0) }));
$('skipTypes').addEventListener('change', (e) => ext.storage.sync.set({ skipTypes: e.target.value }));

$('video').addEventListener('click', async () => {
  const [tab] = await ext.tabs.query({ active: true, currentWindow: true });
  if (!tab?.url) return;
  const r = await ext.runtime.sendMessage({ type: 'media', url: tab.url });
  $('video').textContent = r?.ok ? 'Sent to Haul ✓' : 'Haul isn’t running';
});

// Firefox asks for site access separately; Haul needs it to reach the app and read cookies.
const ORIGINS = { origins: ['<all_urls>'] };
if (ext.permissions?.contains) {
  ext.permissions.contains(ORIGINS).then((ok) => ($('grant').hidden = ok));
  $('grant').addEventListener('click', () => ext.permissions.request(ORIGINS).then((ok) => ($('grant').hidden = ok)));
}

function status() {
  ext.runtime.sendMessage({ type: 'status' }).then((r) => {
    const el = $('status');
    el.textContent = r?.running ? `Connected to Haul ${r.version}` : 'Haul isn’t running';
    el.className = 'status ' + (r?.running ? 'ok' : 'bad');
  });
}
status();
