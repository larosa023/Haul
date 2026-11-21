<script lang="ts">
  import Icon from './Icon.svelte';
  import { app, ui, counts, saveSettings, type Filter } from '../state.svelte';
  import { speed } from '../format.js';

  const FILTERS: { id: Filter; label: string; icon: string }[] = [
    { id: 'all', label: 'All', icon: 'all' },
    { id: 'downloading', label: 'Downloading', icon: 'download' },
    { id: 'seeding', label: 'Seeding', icon: 'seed' },
    { id: 'completed', label: 'Completed', icon: 'done' },
    { id: 'paused', label: 'Paused', icon: 'pause' },
    { id: 'errors', label: 'Errors', icon: 'alert' },
  ];

  const c = $derived(counts());
  const t = $derived(app.totals);
  const alt = $derived(!!app.settings?.altEnabled);
</script>

<aside class="sidebar">
  <div class="brand" data-tauri-drag-region>
    <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
      <path d="M12 3.5v9M8.2 9l3.8 3.8L15.8 9" fill="none" stroke="var(--accent)" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" />
      <path d="M5 14v3.2A1.8 1.8 0 0 0 6.8 19h10.4a1.8 1.8 0 0 0 1.8-1.8V14" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" />
    </svg>
    <span data-tauri-drag-region>Haul</span>
  </div>

  <button class="btn primary new" onclick={() => { ui.view = 'list'; ui.dialog = { kind: 'add' }; }} title="New download (Ctrl+N)">
    <Icon name="plus" size={16} stroke={2.2} />New download
  </button>

  <nav>
    {#each FILTERS as f (f.id)}
      <button
        class="filter"
        class:on={ui.view === 'list' && ui.filter === f.id}
        class:err={f.id === 'errors' && c.errors > 0}
        onclick={() => { ui.view = 'list'; ui.filter = f.id; }}
      >
        <Icon name={f.icon} size={17} />
        <span class="label">{f.label}</span>
        {#if c[f.id]}<span class="count">{c[f.id]}</span>{/if}
      </button>
    {/each}
  </nav>

  <div class="spacer" data-tauri-drag-region></div>

  {#if t?.engine && !t.engine.running && t.engine.error}
    <div class="engine" title={t.engine.error}><Icon name="shield" size={15} /><span>{t.engine.adapterDown ? 'Torrents stopped: adapter down' : 'Torrents unavailable'}</span></div>
  {/if}

  <div class="speeds">
    <div class="sp"><Icon name="download" size={14} /><span>{speed(t?.down ?? 0) || '0 B/s'}</span></div>
    <div class="sp"><Icon name="upload" size={14} /><span>{speed(t?.up ?? 0) || '0 B/s'}</span></div>
    <button
      class="icon-btn alt"
      class:on={alt || t?.alt}
      title={t?.alt && !alt ? 'Scheduled speed limit is on' : alt ? 'Speed limit on (click for full speed)' : 'Limit speed'}
      onclick={() => saveSettings({ altEnabled: !alt })}
    ><Icon name="turtle" size={17} /></button>
  </div>

  <button class="filter settings" class:on={ui.view === 'settings'} onclick={() => (ui.view = ui.view === 'settings' ? 'list' : 'settings')} title="Settings (Ctrl+,)">
    <Icon name="settings" size={17} /><span class="label">Settings</span>
    {#if app.update?.status === 'ready' || app.update?.status === 'available'}<span class="dot"></span>{/if}
  </button>
</aside>

<style>
  .sidebar {
    width: 216px; flex-shrink: 0; display: flex; flex-direction: column; gap: 2px; padding: 0 10px 10px;
    background: var(--sidebar); border-right: 1px solid var(--line);
  }
  .brand { height: 48px; display: flex; align-items: center; gap: 9px; padding: 0 8px; font: 700 15px var(--font-display); letter-spacing: 0.01em; }
  .brand svg { flex-shrink: 0; pointer-events: none; }
  .new { margin: 2px 0 12px; height: 36px; width: 100%; justify-content: flex-start; padding: 0 12px; }
  nav { display: flex; flex-direction: column; gap: 1px; }
  .filter {
    position: relative; display: flex; align-items: center; gap: 11px; height: 36px; padding: 0 10px; border: 0; border-radius: 6px;
    background: none; color: var(--text-2); cursor: pointer; text-align: left; font-weight: 500;
  }
  .filter:hover { background: var(--surface-hover); color: var(--text); }
  .filter.on { background: var(--surface); color: var(--text); }
  .filter.on::before { content: ''; position: absolute; left: 0; top: 10px; bottom: 10px; width: 3px; border-radius: 3px; background: var(--accent); }
  .filter.on :global(svg) { color: var(--accent); }
  .filter.err .count { background: color-mix(in srgb, var(--danger) 22%, transparent); color: var(--danger); }
  .label { flex: 1; }
  .count { font-size: 11.5px; font-weight: 600; min-width: 22px; text-align: center; padding: 1px 6px; border-radius: 9px; background: var(--surface-strong); color: var(--text-2); }
  .spacer { flex: 1; }
  .engine { display: flex; gap: 7px; align-items: center; font-size: 12px; color: var(--warn); padding: 6px 10px; }
  .speeds { display: flex; align-items: center; gap: 10px; padding: 8px 4px 8px 10px; border-top: 1px solid var(--line); margin-top: 4px; }
  .sp { display: flex; align-items: center; gap: 5px; font-size: 12px; color: var(--text-2); font-variant-numeric: tabular-nums; min-width: 0; flex: 1; white-space: nowrap; }
  .alt { width: 30px; height: 30px; }
  .settings .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--accent); }
</style>
