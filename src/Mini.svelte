<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from './lib/api';
  import { app, history, load, listenAll } from './lib/state.svelte';
  import { setSpeedBits } from './lib/format.js';
  import { applyTheme, reloadThemes, refreshSystem } from './lib/theme.svelte';
  import { speed, percent, eta } from './lib/format.js';
  import Icon from './lib/components/Icon.svelte';
  import SpeedGraph from './lib/components/SpeedGraph.svelte';

  onMount(() => {
    document.documentElement.dataset.mini = 'true';
    listenAll();
    void (async () => {
      await Promise.all([refreshSystem(), reloadThemes()]);
      await load().catch(() => {});
      requestAnimationFrame(() => requestAnimationFrame(() => void api.uiReady().catch(() => {})));
    })();
  });

  $effect(() => {
    if (app.settings) setSpeedBits(app.settings.speedBits);
  });

  $effect(() => {
    if (app.settings) applyTheme({ theme: app.settings.theme, accent: app.settings.accent, compactRows: false }, true);
  });

  const ACTIVE = ['queued', 'metadata', 'checking', 'downloading'];
  const active = $derived(app.order.map((id) => app.items[id]).filter((i) => i && ACTIVE.includes(i.status)));
  const paused = $derived(app.order.map((id) => app.items[id]).filter((i) => i && i.status === 'paused'));
  const shown = $derived(active.slice(0, 5));
  const down = $derived(app.totals?.down ?? 0);
  const up = $derived(app.totals?.up ?? 0);

  async function pauseAll() {
    await api.pause(active.map((i) => i.id)).catch(() => {});
  }
  async function resumeAll() {
    await api.resume(paused.map((i) => i.id)).catch(() => {});
  }
  async function drag(e: MouseEvent) {
    if (e.button !== 0 || (e.target as HTMLElement).closest('button')) return;
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    void getCurrentWindow().startDragging();
  }
</script>

<div class="mini" role="presentation" onmousedown={drag}>
  <div class="spark"><SpeedGraph data={history.total} height={46} dot={false} /></div>
  <header>
    <div class="speed">
      <span class="d"><Icon name="download" size={14} stroke={2.2} />{speed(down) || '0 B/s'}</span>
      {#if up}<span class="u"><Icon name="upload" size={13} stroke={2.2} />{speed(up)}</span>{/if}
    </div>
    <button class="ib" title="Open Haul" onclick={() => api.showWindow()}><Icon name="open" size={15} /></button>
    <button class="ib" title="Close the mini window" onclick={() => api.toggleMini()}><Icon name="x" size={15} /></button>
  </header>

  <div class="list">
    {#each shown as it (it.id)}
      <div class="it">
        <div class="top"><span class="nm" title={it.name}>{it.name}</span><span class="pc">{it.size ? `${Math.floor(percent(it.done, it.size))}%` : ''}</span></div>
        <div class="bar"><span style="width: {percent(it.done, it.size)}%"></span></div>
        <div class="sub"><span>{speed(it.speedDown) || it.note || (it.status === 'queued' ? 'Queued' : '')}</span><span>{eta(it.eta)}</span></div>
      </div>
    {:else}
      <div class="empty"><Icon name="done" size={22} /><span>{paused.length ? `${paused.length} paused` : 'Nothing downloading'}</span></div>
    {/each}
    {#if active.length > shown.length}<div class="more">+{active.length - shown.length} more</div>{/if}
  </div>

  <footer>
    <span class="count">{active.length} active</span>
    {#if active.length}
      <button class="btn" onclick={pauseAll}><Icon name="pause" size={14} />Pause all</button>
    {:else if paused.length}
      <button class="btn primary" onclick={resumeAll}><Icon name="play" size={14} />Resume all</button>
    {/if}
  </footer>
</div>

<style>
  :global(html[data-mini='true']), :global(html[data-mini='true'] body) { background: transparent; }
  .mini {
    position: fixed; inset: 6px; display: flex; flex-direction: column; overflow: hidden;
    border-radius: 12px; border: 1px solid var(--line); background: var(--solid-bg, var(--bg));
    box-shadow: 0 6px 18px rgb(0 0 0 / 0.35); cursor: default;
  }
  .spark { position: absolute; left: 0; right: 0; top: 0; opacity: 0.5; pointer-events: none; }
  header { position: relative; display: flex; align-items: center; gap: 4px; padding: 8px 8px 6px 12px; }
  .speed { flex: 1; display: flex; align-items: baseline; gap: 10px; font-variant-numeric: tabular-nums; }
  .speed span { display: inline-flex; align-items: center; gap: 4px; }
  .d { font: 600 16px var(--font-display); color: var(--text); }
  .d :global(svg) { color: var(--accent); }
  .u { font-size: 12px; color: var(--text-2); }
  .ib { width: 26px; height: 26px; display: grid; place-items: center; border: 0; border-radius: 6px; background: transparent; color: var(--text-2); cursor: pointer; }
  .ib:hover { background: var(--surface-hover); color: var(--text); }
  .list { flex: 1; overflow: hidden; padding: 0 10px; display: flex; flex-direction: column; gap: 8px; }
  .it { display: flex; flex-direction: column; gap: 3px; }
  .top, .sub { display: flex; justify-content: space-between; gap: 8px; font-size: 12px; }
  .nm { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .pc { color: var(--text-2); font-variant-numeric: tabular-nums; }
  .sub { font-size: 11px; color: var(--text-3); font-variant-numeric: tabular-nums; }
  .bar { height: 4px; border-radius: 2px; background: var(--surface-strong); overflow: hidden; }
  .bar span { display: block; height: 100%; background: var(--accent); transition: width 900ms linear; }
  .empty { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 6px; color: var(--text-3); font-size: 12.5px; }
  .more { font-size: 11.5px; color: var(--text-3); text-align: center; }
  footer { display: flex; align-items: center; gap: 8px; padding: 8px 10px; border-top: 1px solid var(--line); }
  .count { flex: 1; font-size: 12px; color: var(--text-2); }
  footer .btn { height: 28px; padding: 0 10px; font-size: 12.5px; }
</style>
