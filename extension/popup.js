const DEFAULTS = { enabled: true, port: 17771, minSizeKb: 0, skipTypes: '' };
const $ = (id) => document.getElementById(id);

chrome.storage.sync.get(DEFAULTS).then((s) => {
  $('enabled').checked = s.enabled;
  $('port').value = s.port;
  $('minSizeKb').value = s.minSizeKb;
  $('skipTypes').value = s.skipTypes;
});

$('enabled').addEventListener('change', (e) => chrome.storage.sync.set({ enabled: e.target.checked }));
$('port').addEventListener('change', (e) => chrome.storage.sync.set({ port: Number(e.target.value) || 17771 }).then(status));
$('minSizeKb').addEventListener('change', (e) => chrome.storage.sync.set({ minSizeKb: Math.max(0, Number(e.target.value) || 0) }));
$('skipTypes').addEventListener('change', (e) => chrome.storage.sync.set({ skipTypes: e.target.value }));

function status() {
  chrome.runtime.sendMessage({ type: 'status' }).then((r) => {
    const el = $('status');
    el.textContent = r?.running ? `Connected to Haul ${r.version}` : 'Haul isn’t running';
    el.className = 'status ' + (r?.running ? 'ok' : 'bad');
  });
}
status();
