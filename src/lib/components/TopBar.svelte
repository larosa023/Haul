<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import WindowControls from './WindowControls.svelte';
  import { api } from '../api';
  import { app, ui, selection, selectedItems, refreshOrder, toast, errorText } from '../state.svelte';

  let search: HTMLInputElement | undefined = $state();
  const sel = $derived(selectedItems());
  const canPause = $derived(sel.some((i) => !i.paused && i.status !== 'completed' && i.status !== 'error'));
  const canResume = $derived(sel.some((i) => i.paused || i.status === 'error' || (i.kind === 'torrent' && i.status === 'completed')));

  onMount(() => {
    const f = () => search?.focus();
    window.addEventListener('haul:focus-search', f);
    return () => window.removeEventListener('haul:focus-search', f);
  });

  async function run(fn: () => Promise<unknown>) {
    try {
      await fn();
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }

  const ids = () => [...selection];
</script>

<header class="top" data-tauri-drag-region>
  {#if ui.view === 'settings' || ui.view === 'search'}
    <button class="icon-btn" title="Back (Esc)" onclick={() => (ui.view = 'list')}><Icon name="chevronLeft" /></button>
    <h1 data-tauri-drag-region>{ui.view === 'search' ? 'Torrent search' : 'Settings'}</h1>
  {:else}
    <div class="search">
      <Icon name="search" size={15} />
      <input bind:this={search} bind:value={ui.search} placeholder="Search" spellcheck="false" aria-label="Search downloads" />
      {#if ui.search}<button class="clear" aria-label="Clear search" onclick={() => (ui.search = '')}><Icon name="x" size={14} /></button>{/if}
    </div>
    <div class="actions">
      <button class="icon-btn" title="Resume (Space)" disabled={!canResume} onclick={() => run(() => (sel.every((i) => i.status === 'error') ? api.retry(ids()) : api.resume(ids())))}><Icon name="play" size={16} /></button>
      <button class="icon-btn" title="Pause (Space)" disabled={!canPause} onclick={() => run(() => api.pause(ids()))}><Icon name="pause" size={16} /></button>
      <button class="icon-btn" title="Remove (Del)" disabled={!sel.length} onclick={() => (ui.dialog = { kind: 'remove', ids: ids() })}><Icon name="trash" size={16} /></button>
      <span class="sep"></span>
      <button class="icon-btn" title="Move up in queue (Alt+↑)" disabled={!sel.length} onclick={() => run(async () => refreshOrder(await api.move(ids(), 'up')))}><Icon name="up" size={16} /></button>
      <button class="icon-btn" title="Move down in queue (Alt+↓)" disabled={!sel.length} onclick={() => run(async () => refreshOrder(await api.move(ids(), 'down')))}><Icon name="down" size={16} /></button>
      <span class="sep"></span>
      <button class="icon-btn" title="Show in folder (Shift+Enter)" disabled={sel.length !== 1} onclick={() => run(() => api.reveal(sel[0].id))}><Icon name="folder" size={16} /></button>
      <button class="icon-btn" class:on={ui.details} title="Details (F2)" onclick={() => (ui.details = !ui.details)}><Icon name="info" size={16} /></button>
    </div>
  {/if}
  <div class="drag" data-tauri-drag-region></div>
  {#if !app.settings?.nativeTitleBar}<div class="wc"><WindowControls /></div>{/if}
</header>

<style>
  .top { height: 48px; flex-shrink: 0; display: flex; align-items: center; gap: 10px; padding-left: 14px; user-select: none; }
  h1 { margin: 0; font: 600 16px var(--font-display); }
  .search {
    display: flex; align-items: center; gap: 8px; width: min(340px, 34vw); height: 32px; padding: 0 8px 0 11px; border-radius: 6px;
    background: var(--surface); border: 1px solid var(--line); color: var(--text-3);
  }
  .search:focus-within { border-color: var(--accent); color: var(--text-2); }
  .search input { flex: 1; min-width: 0; border: 0; outline: 0; background: none; color: var(--text); }
  .search input::placeholder { color: var(--text-3); }
  .clear { border: 0; background: none; color: var(--text-3); cursor: pointer; display: grid; place-items: center; padding: 2px; border-radius: 4px; }
  .clear:hover { color: var(--text); background: var(--surface-hover); }
  .actions { display: flex; align-items: center; gap: 2px; }
  .sep { width: 1px; height: 18px; background: var(--line); margin: 0 4px; }
  .drag { flex: 1; align-self: stretch; }
  .wc { align-self: stretch; display: flex; }
</style>
