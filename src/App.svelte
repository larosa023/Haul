<script lang="ts">
  import { onMount } from 'svelte';
  import { api, isTauri } from './lib/api';
  import { app, ui, selection, load, listenAll, nextPending, visibleItems, selectedItems, selectOnly, toast, errorText, refreshOrder } from './lib/state.svelte';
  import { setSpeedBits } from './lib/format.js';
  import { applyTheme, reloadThemes, refreshSystem } from './lib/theme.svelte';
  import Sidebar from './lib/components/Sidebar.svelte';
  import TopBar from './lib/components/TopBar.svelte';
  import DownloadList from './lib/components/DownloadList.svelte';
  import Details from './lib/components/Details.svelte';
  import AddDialog from './lib/components/AddDialog.svelte';
  import TorrentDialog from './lib/components/TorrentDialog.svelte';
  import RemoveDialog from './lib/components/RemoveDialog.svelte';
  import MediaDialog from './lib/components/MediaDialog.svelte';
  import GrabDialog from './lib/components/GrabDialog.svelte';
  import HfDialog from './lib/components/HfDialog.svelte';
  import Banners from './lib/components/Banners.svelte';
  import Toasts from './lib/components/Toasts.svelte';
  import ContextMenu from './lib/components/ContextMenu.svelte';
  import Settings from './routes/Settings.svelte';
  import Search from './routes/Search.svelte';

  onMount(() => {
    listenAll();
    void (async () => {
      await Promise.all([refreshSystem(), reloadThemes()]);
      try {
        await load();
      } catch (e) {
        toast(errorText(e), 'error', 10000);
      }
      // Show the window only once the first frame is drawn (no white flash).
      requestAnimationFrame(() => requestAnimationFrame(() => void api.uiReady().catch(() => {})));
    })();
    // Follow Windows' light/dark and accent changes.
    const t = setInterval(refreshSystem, 15000);
    const mq = matchMedia('(prefers-color-scheme: light)');
    mq.addEventListener('change', refreshSystem);
    return () => {
      clearInterval(t);
      mq.removeEventListener('change', refreshSystem);
    };
  });

  // Live theming: any change to theme, accent or the system colors applies at once.
  $effect(() => {
    if (app.settings) setSpeedBits(app.settings.speedBits);
    document.documentElement.dataset.progress = app.settings?.progressStyle || 'glow';
  });

  $effect(() => {
    if (app.settings) applyTheme({ theme: app.settings.theme, accent: app.settings.accent, compactRows: app.settings.compactRows });
  });

  // When a dialog closes, show the next torrent waiting for its files.
  $effect(() => {
    if (app.ready && !ui.dialog && isTauri) void nextPending();
  });

  $effect(() => {
    if (app.settings && isTauri) void api.setNativeFrame(app.settings.nativeTitleBar).catch(() => {});
  });

  function typing(e: KeyboardEvent) {
    const el = e.target as HTMLElement;
    return /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName) || el.isContentEditable;
  }

  async function togglePause() {
    const items = selectedItems();
    if (!items.length) return;
    // Mixed selection: if anything is running, pause everything; otherwise resume.
    const anyActive = items.some((i) => !i.paused && i.status !== 'completed' && i.status !== 'error');
    const ids = items.map((i) => i.id);
    if (anyActive) await api.pause(ids);
    else if (items.every((i) => i.status === 'error')) await api.retry(ids);
    else await api.resume(ids);
  }

  function moveFocus(delta: number, extend: boolean) {
    const list = visibleItems();
    if (!list.length) return;
    let idx = list.findIndex((i) => i.id === ui.focus);
    idx = idx < 0 ? 0 : Math.max(0, Math.min(list.length - 1, idx + delta));
    const id = list[idx].id;
    if (extend) {
      const a = list.findIndex((i) => i.id === ui.anchor);
      const [lo, hi] = a < 0 ? [idx, idx] : [Math.min(a, idx), Math.max(a, idx)];
      selection.clear();
      for (let i = lo; i <= hi; i++) selection.add(list[i].id);
      ui.focus = id;
    } else {
      selectOnly(id);
    }
    window.dispatchEvent(new CustomEvent('haul:scroll-to', { detail: id }));
  }

  async function onKey(e: KeyboardEvent) {
    const ctrl = e.ctrlKey || e.metaKey;
    if (ctrl && e.key === ',') {
      e.preventDefault();
      ui.view = ui.view === 'settings' ? 'list' : 'settings';
      return;
    }
    if (e.key === 'Escape') {
      if (ui.dialog) return; // dialogs close themselves
      if (ui.view === 'settings' || ui.view === 'search') ui.view = 'list';
      else if (ui.search) ui.search = '';
      else selection.clear();
      return;
    }
    if (ui.dialog) return;
    if (ctrl && e.key.toLowerCase() === 'n') {
      e.preventDefault();
      ui.view = 'list';
      ui.dialog = { kind: 'add' };
      return;
    }
    if (ctrl && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      ui.view = 'search';
      return;
    }
    if (ctrl && e.key.toLowerCase() === 'o') {
      e.preventDefault();
      window.dispatchEvent(new Event('haul:open-torrent'));
      return;
    }
    if (ctrl && e.key.toLowerCase() === 'f') {
      e.preventDefault();
      ui.view = 'list';
      window.dispatchEvent(new Event('haul:focus-search'));
      return;
    }
    if (ui.view !== 'list' || typing(e)) return;
    if (ctrl && e.key.toLowerCase() === 'a') {
      e.preventDefault();
      for (const it of visibleItems()) selection.add(it.id);
    } else if (e.key === ' ') {
      e.preventDefault();
      await togglePause().catch((err) => toast(errorText(err), 'error'));
    } else if (e.key === 'Delete') {
      e.preventDefault();
      const ids = [...selection];
      if (ids.length) ui.dialog = { kind: 'remove', ids };
    } else if (e.key === 'Enter') {
      const it = selectedItems()[0];
      if (it) void (e.shiftKey ? api.reveal(it.id) : api.open(it.id));
    } else if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
      e.preventDefault();
      const ids = [...selection];
      if (ids.length) await refreshOrder(await api.move(ids, e.key === 'ArrowUp' ? (ctrl ? 'top' : 'up') : ctrl ? 'bottom' : 'down'));
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      moveFocus(e.key === 'ArrowDown' ? 1 : -1, e.shiftKey);
    } else if (e.key === 'Home' || e.key === 'End') {
      e.preventDefault();
      moveFocus(e.key === 'Home' ? -1e9 : 1e9, e.shiftKey);
    } else if (e.key === 'F2' || (ctrl && e.key.toLowerCase() === 'i')) {
      ui.details = !ui.details;
    }
  }

  // Dropping .torrent files or links anywhere adds them.
  async function onDrop(e: DragEvent) {
    e.preventDefault();
    const dt = e.dataTransfer;
    if (!dt) return;
    for (const f of Array.from(dt.files)) {
      if (f.name.toLowerCase().endsWith('.torrent')) {
        const bytes = Array.from(new Uint8Array(await f.arrayBuffer()));
        await api.addTorrent({ bytes }).catch((err) => toast(errorText(err), 'error'));
      }
    }
    const text = dt.getData('text/uri-list') || dt.getData('text/plain');
    if (text && !dt.files.length) ui.dialog = { kind: 'add', url: text.split('\n')[0].trim() };
  }
  const ACTIVE = ['downloading', 'queued', 'metadata', 'checking'];
  const busy = $derived(app.order.some((id) => ACTIVE.includes(app.items[id]?.status)) || (app.totals?.up ?? 0) > 0 || (app.totals?.down ?? 0) > 0);
</script>

<svelte:window onkeydown={onKey} ondragover={(e) => e.preventDefault()} ondrop={onDrop} />

{#if app.ready && app.settings}
  <div class="shell" class:custom-chrome={!app.settings.nativeTitleBar}>
    <Sidebar />
    <main>
      <TopBar />
      {#if ui.view === 'settings'}
        <Settings />
      {:else if ui.view === 'search'}
        <Search />
      {:else}
        <Banners />
        <DownloadList />
        <!-- The details panel shows a selection, or the live overview while something is moving;
             with nothing selected and nothing happening it stays out of the way. -->
        {#if ui.details && (selection.size || busy)}<Details />{/if}
      {/if}
    </main>
  </div>
  {#if ui.dialog?.kind === 'add'}<AddDialog url={ui.dialog.url} request={ui.dialog.request} />{/if}
  {#if ui.dialog?.kind === 'torrent'}<TorrentDialog meta={ui.dialog.meta} />{/if}
  {#if ui.dialog?.kind === 'remove'}<RemoveDialog ids={ui.dialog.ids} />{/if}
  {#if ui.dialog?.kind === 'media'}{#key ui.dialog.url}<MediaDialog url={ui.dialog.url} referrer={ui.dialog.referrer} />{/key}{/if}
  {#if ui.dialog?.kind === 'grab'}<GrabDialog url={ui.dialog.url} />{/if}
  {#if ui.dialog?.kind === 'hf'}<HfDialog input={ui.dialog.input} />{/if}
  <ContextMenu />
{/if}
<Toasts />

<style>
  .shell { display: flex; height: 100%; }
  main { flex: 1; min-width: 0; display: flex; flex-direction: column; background: var(--bg); }
  :global(html[data-mode]) .shell { background: transparent; }
</style>
