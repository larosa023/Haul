// Alt+click (or Alt+Enter) on a link lets the browser download it itself instead of Haul.
addEventListener('click', (e) => { if (e.altKey) chrome.runtime.sendMessage({ type: 'bypass' }).catch(() => {}); }, true);
addEventListener('keydown', (e) => { if (e.altKey && e.key === 'Enter') chrome.runtime.sendMessage({ type: 'bypass' }).catch(() => {}); }, true);
