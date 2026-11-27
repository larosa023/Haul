<script lang="ts">
  import { onMount } from 'svelte';
  import Modal from './Modal.svelte';
  import Icon from './Icon.svelte';
  import { api, type GrabLink } from '../api';
  import { app, ui, toast, errorText } from '../state.svelte';
  import { bytes } from '../format.js';

  let { url = '' }: { url?: string } = $props();

  // svelte-ignore state_referenced_locally
  let page = $state(url);
  let links = $state<GrabLink[]>([]);
  let picked = $state(new Set<string>());
  let loading = $state(false);
  let error = $state('');
  let busy = $state(false);
  let cat = $state('');
  let text = $state('');
  let minMb = $state(0);
  let showPages = $state(false);

  const cats = $derived.by(() => {
    const m = new Map<string, number>();
    for (const l of links) if (showPages || l.category !== 'Page') m.set(l.category, (m.get(l.category) ?? 0) + 1);
    return [...m.entries()].sort((a, b) => b[1] - a[1]);
  });
  const shown = $derived(
    links.filter(
      (l) =>
        (showPages || l.category !== 'Page') &&
        (!cat || l.category === cat) &&
        (!minMb || l.size >= minMb * 1048576) &&
        (!text || `${l.name} ${l.text} ${l.url}`.toLowerCase().includes(text.toLowerCase())),
    ),
  );
  const pickedSize = $derived(links.reduce((s, l) => s + (picked.has(l.url) ? l.size : 0), 0));

  async function grab() {
    const u = page.trim();
    if (!u || loading) return;
    loading = true;
    error = '';
    links = [];
    try {
      links = await api.grabLinks(u);
      // Pick the files (not other pages) to start with.
      picked = new Set(links.filter((l) => l.category !== 'Page').map((l) => l.url));
      if (!links.length) error = 'No links found on that page.';
    } catch (e) {
      error = errorText(e);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    if (url) void grab();
  });

  function toggle(u: string) {
    const n = new Set(picked);
    if (n.has(u)) n.delete(u);
    else n.add(u);
    picked = n;
  }

  function setShown(v: boolean) {
    const n = new Set(picked);
    for (const l of shown) v ? n.add(l.url) : n.delete(l.url);
    picked = n;
  }

  async function add() {
    busy = true;
    let n = 0;
    try {
      for (const l of links) {
        if (!picked.has(l.url)) continue;
        try {
          if (l.category === 'Torrent') await api.addTorrent({ source: l.url, direct: true });
          else await api.addHttp({ url: l.url, referrer: page.trim() });
          n++;
        } catch (e) {
          toast(`${l.name}: ${errorText(e)}`, 'error', 7000);
        }
      }
      toast(`Queued ${n} download${n === 1 ? '' : 's'}`, 'success');
      ui.dialog = null;
    } finally {
      busy = false;
    }
  }

  const ICON: Record<string, string> = { Video: 'video', Music: 'music', Archives: 'archive', Torrent: 'magnet', Page: 'globe' };
</script>

<Modal title="Grab links from a page" width={760} onclose={() => (ui.dialog = null)}>
  <div class="grab">
    <div class="bar">
      <!-- svelte-ignore a11y_autofocus -->
      <input class="input mono" bind:value={page} placeholder="https://… page with files on it" spellcheck="false" autofocus onkeydown={(e) => e.key === 'Enter' && grab()} />
      <button class="btn primary" disabled={loading || !page.trim()} onclick={grab}>{#if loading}<span class="spinner"></span>{/if}Find links</button>
    </div>

    {#if links.length}
      <div class="filters">
        <div class="cats">
          <button class="cc" class:on={!cat} onclick={() => (cat = '')}>All</button>
          {#each cats as [c, n] (c)}<button class="cc" class:on={cat === c} onclick={() => (cat = c)}>{c} <small>{n}</small></button>{/each}
        </div>
        <div class="more">
          <input class="input" bind:value={text} placeholder="Filter by name" spellcheck="false" />
          <label class="min">At least <input class="input" type="number" min="0" bind:value={minMb} /> MB</label>
          <label class="pages"><input type="checkbox" bind:checked={showPages} /> Show page links</label>
        </div>
      </div>
      <div class="list" role="list">
        {#each shown as l (l.url)}
          <label class="link" role="listitem" title={l.url}>
            <input type="checkbox" checked={picked.has(l.url)} onchange={() => toggle(l.url)} />
            <Icon name={ICON[l.category] ?? 'file'} size={15} />
            <span class="nm">{l.name}{#if l.text && l.text !== l.name}<span class="tx"> · {l.text}</span>{/if}</span>
            <span class="ct">{l.category}</span>
            <span class="sz">{l.size ? bytes(l.size) : ''}</span>
          </label>
        {:else}
          <p class="muted empty">Nothing matches the filters.</p>
        {/each}
      </div>
    {:else if error}
      <p class="bad"><Icon name="alert" size={15} /> {error}</p>
    {:else if !loading}
      <p class="muted hint">Haul reads the page and lists every file it links to. Filter them by type, size or name, then pick what to download.</p>
    {/if}
  </div>

  {#snippet footer()}
    {#if links.length}
      <button class="btn ghost" onclick={() => setShown(true)}>Select shown</button>
      <button class="btn ghost" onclick={() => setShown(false)}>Clear shown</button>
      <span class="muted sel">{picked.size} picked{pickedSize ? ` · ${bytes(pickedSize)}` : ''}</span>
    {/if}
    <span class="grow"></span>
    <button class="btn" onclick={() => (ui.dialog = null)}>Cancel</button>
    <button class="btn primary" disabled={!picked.size || busy || !app.settings} onclick={add}>{#if busy}<span class="spinner"></span>{/if}Download {picked.size || ''}</button>
  {/snippet}
</Modal>

<style>
  .grab { display: flex; flex-direction: column; gap: 12px; }
  .bar { display: flex; gap: 8px; }
  .bar .input { flex: 1; }
  .filters { display: flex; flex-direction: column; gap: 8px; }
  .cats { display: flex; flex-wrap: wrap; gap: 6px; }
  .cc { border: 1px solid var(--line); background: var(--surface); padding: 4px 10px; border-radius: 14px; cursor: pointer; font-size: 12.5px; }
  .cc small { color: var(--text-3); margin-left: 3px; }
  .cc.on { background: var(--accent); color: var(--on-accent); border-color: transparent; }
  .cc.on small { color: inherit; opacity: 0.8; }
  .more { display: flex; gap: 10px; align-items: center; font-size: 12.5px; }
  .more > .input { flex: 1; }
  .min { display: flex; align-items: center; gap: 6px; white-space: nowrap; }
  .min .input { width: 70px; }
  .pages { display: flex; align-items: center; gap: 6px; white-space: nowrap; cursor: pointer; }
  .list { height: 320px; overflow: auto; border: 1px solid var(--line); border-radius: 8px; }
  .link { display: grid; grid-template-columns: 20px 18px 1fr 90px 72px; align-items: center; gap: 8px; height: 32px; padding: 0 10px; font-size: 12.5px; border-bottom: 1px solid var(--line); cursor: pointer; content-visibility: auto; contain-intrinsic-size: auto 32px; }
  .link:hover { background: var(--surface-hover); }
  .nm { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .tx { color: var(--text-3); }
  .ct { color: var(--text-2); }
  .sz { text-align: right; color: var(--text-2); font-variant-numeric: tabular-nums; }
  .empty, .hint { margin: 0; padding: 14px; font-size: 13px; }
  .bad { color: var(--danger); display: flex; align-items: center; gap: 6px; margin: 0; }
  .sel { font-size: 12.5px; }
  .grow { flex: 1; }
</style>
