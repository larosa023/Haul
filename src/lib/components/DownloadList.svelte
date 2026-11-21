<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import { api, type Item } from '../api';
  import { app, ui, selection, visibleItems, selectOnly } from '../state.svelte';
  import { bytes, speed, eta, percent, statusLabel, linkKind } from '../format.js';

  const items = $derived(visibleItems());
  const compact = $derived(!!app.settings?.compactRows);
  const rowH = $derived(compact ? 34 : 44);

  let scroller: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let height = $state(400);

  // Only the rows on screen are drawn, so hundreds of torrents stay smooth.
  const first = $derived(Math.max(0, Math.floor(scrollTop / rowH) - 6));
  const last = $derived(Math.min(items.length, Math.ceil((scrollTop + height) / rowH) + 6));
  const slice = $derived(items.slice(first, last));

  onMount(() => {
    const ro = new ResizeObserver(() => (height = scroller?.clientHeight ?? 400));
    if (scroller) ro.observe(scroller);
    const onScrollTo = (e: Event) => {
      const id = (e as CustomEvent<number>).detail;
      const idx = items.findIndex((i) => i.id === id);
      if (idx < 0 || !scroller) return;
      const top = idx * rowH;
      if (top < scroller.scrollTop) scroller.scrollTop = top;
      else if (top + rowH > scroller.scrollTop + scroller.clientHeight) scroller.scrollTop = top + rowH - scroller.clientHeight;
    };
    window.addEventListener('haul:scroll-to', onScrollTo);
    return () => {
      ro.disconnect();
      window.removeEventListener('haul:scroll-to', onScrollTo);
    };
  });

  function click(e: MouseEvent, it: Item) {
    if (e.shiftKey) {
      const a = items.findIndex((i) => i.id === ui.anchor);
      const b = items.findIndex((i) => i.id === it.id);
      if (a >= 0) {
        if (!e.ctrlKey) selection.clear();
        for (let i = Math.min(a, b); i <= Math.max(a, b); i++) selection.add(items[i].id);
        ui.focus = it.id;
        return;
      }
    }
    if (e.ctrlKey || e.metaKey) {
      if (selection.has(it.id)) selection.delete(it.id);
      else selection.add(it.id);
      ui.anchor = it.id;
      ui.focus = it.id;
      return;
    }
    selectOnly(it.id);
  }

  function dbl(it: Item) {
    if (it.status === 'completed' || it.status === 'seeding') void api.open(it.id);
    else ui.details = true;
  }

  function menu(e: MouseEvent, it: Item) {
    e.preventDefault();
    if (!selection.has(it.id)) selectOnly(it.id);
    window.dispatchEvent(new CustomEvent('haul:menu', { detail: { x: e.clientX, y: e.clientY } }));
  }

  function onPaste(e: ClipboardEvent) {
    const el = e.target as HTMLElement;
    if (ui.dialog || /^(INPUT|TEXTAREA)$/.test(el.tagName)) return;
    const text = e.clipboardData?.getData('text') ?? '';
    if (linkKind(text)) {
      e.preventDefault();
      ui.dialog = { kind: 'add', url: text.trim() };
    }
  }

  function tone(it: Item) {
    if (it.status === 'error') return 'err';
    if (it.status === 'seeding') return 'seed';
    if (it.status === 'completed') return 'done';
    if (it.status === 'paused' || it.status === 'queued') return 'idle';
    return 'run';
  }

  function sub(it: Item) {
    if (it.status === 'error') return it.error;
    if (it.note) return it.note;
    if (it.hashState === 'ok') return 'SHA-256 verified';
    if (it.kind === 'torrent' && (it.status === 'downloading' || it.status === 'seeding')) return `${it.peers} peers${it.speedUp ? ` · ↑ ${speed(it.speedUp)}` : ''}`;
    return it.category || (it.kind === 'torrent' ? 'Torrent' : '');
  }
</script>

<svelte:window onpaste={onPaste} />

<div class="list" role="grid" aria-label="Downloads" aria-rowcount={items.length}>
  <div class="head" role="row">
    <span class="c-name">Name</span>
    <span class="c-size">Size</span>
    <span class="c-prog">Progress</span>
    <span class="c-speed">Speed</span>
    <span class="c-eta">Time left</span>
    <span class="c-status">Status</span>
  </div>
  <div class="scroll" bind:this={scroller} onscroll={() => (scrollTop = scroller?.scrollTop ?? 0)}>
    {#if !items.length}
      <div class="empty">
        {#if ui.search}
          <p>Nothing matches “{ui.search}”.</p>
        {:else if ui.filter === 'all'}
          <div class="hero"><Icon name="download" size={30} /></div>
          <p class="big">Nothing here yet</p>
          <p class="muted">Press <span class="kbd">Ctrl</span> <span class="kbd">N</span>, paste a link or magnet, or drop a .torrent file here.</p>
        {:else}
          <p class="muted">No {ui.filter === 'errors' ? 'errors' : `${ui.filter} downloads`}.</p>
        {/if}
      </div>
    {:else}
      <div class="spacer" style="height: {items.length * rowH}px">
        {#each slice as it, i (it.id)}
          {@const pct = percent(it.done, it.size)}
          <div
            class="row {tone(it)}"
            class:sel={selection.has(it.id)}
            class:focus={ui.focus === it.id}
            class:compact
            role="row"
            tabindex="-1"
            aria-selected={selection.has(it.id)}
            style="top: {(first + i) * rowH}px; height: {rowH}px"
            onclick={(e) => click(e, it)}
            ondblclick={() => dbl(it)}
            oncontextmenu={(e) => menu(e, it)}
            onkeydown={() => {}}
          >
            <span class="c-name">
              <span class="kind"><Icon name={it.kind === 'torrent' ? 'magnet' : 'file'} size={16} /></span>
              <span class="names">
                <span class="title" title={it.name}>{it.name}</span>
                {#if !compact}<span class="subline" class:errtext={it.status === 'error'} title={sub(it)}>{sub(it)}</span>{/if}
              </span>
              {#if it.priority > 0}<span class="chip pri" title="High priority">High</span>{/if}
              {#if it.limitDown || it.limitUp}<span class="lim" title="Has its own speed limit"><Icon name="gauge" size={13} /></span>{/if}
            </span>
            <span class="c-size">{it.size ? bytes(it.size) : '—'}</span>
            <span class="c-prog">
              <span class="bar"><span class="fill" style="width: {it.status === 'metadata' ? 0 : pct}%"></span>{#if it.status === 'metadata' || it.status === 'checking'}<span class="indet"></span>{/if}</span>
              <span class="pct">{it.status === 'metadata' ? '' : `${pct >= 99.95 || pct === 0 ? pct.toFixed(0) : pct.toFixed(1)}%`}</span>
            </span>
            <span class="c-speed">{it.status === 'downloading' ? speed(it.speedDown) : it.status === 'seeding' ? speed(it.speedUp) : ''}</span>
            <span class="c-eta">{it.status === 'downloading' ? eta(it.eta) : ''}</span>
            <span class="c-status"><span class="dot"></span>{statusLabel(it)}</span>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .list { flex: 1; min-height: 120px; display: flex; flex-direction: column; margin: 0 12px; border-radius: 8px 8px 0 0; background: var(--surface); border: 1px solid var(--line); border-bottom: 0; overflow: hidden; }
  .head, .row { display: grid; grid-template-columns: minmax(200px, 1fr) 84px minmax(120px, 170px) 92px 84px 116px; align-items: center; gap: 10px; padding: 0 14px 0 12px; }
  .head { height: 32px; flex-shrink: 0; font-size: 12px; font-weight: 600; color: var(--text-3); border-bottom: 1px solid var(--line); }
  .scroll { flex: 1; overflow-y: auto; overflow-x: hidden; position: relative; }
  .spacer { position: relative; }
  .row { position: absolute; left: 0; right: 0; cursor: default; border-bottom: 1px solid var(--line); font-variant-numeric: tabular-nums; }
  .row:hover { background: var(--surface-hover); }
  .row.sel { background: color-mix(in srgb, var(--accent) 16%, var(--surface)); }
  .row.sel.focus { box-shadow: inset 2px 0 var(--accent); }
  .c-name { display: flex; align-items: center; gap: 10px; min-width: 0; }
  .kind { color: var(--text-3); display: grid; place-items: center; flex-shrink: 0; }
  .run .kind { color: var(--accent); }
  .names { display: flex; flex-direction: column; min-width: 0; flex: 1; line-height: 1.3; }
  .title { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-weight: 500; }
  .subline { font-size: 11.5px; color: var(--text-3); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .errtext { color: var(--danger); }
  .pri { background: color-mix(in srgb, var(--accent) 22%, transparent); color: var(--accent); }
  .lim { color: var(--text-3); display: grid; }
  .c-size, .c-speed, .c-eta { font-size: 12.5px; color: var(--text-2); white-space: nowrap; }
  .head .c-size, .head .c-speed, .head .c-eta, .row .c-size, .row .c-speed, .row .c-eta { text-align: right; }
  .c-prog { display: flex; align-items: center; gap: 8px; }
  .bar { position: relative; flex: 1; height: 5px; border-radius: 3px; background: var(--surface-strong); overflow: hidden; }
  .fill { position: absolute; inset: 0 auto 0 0; background: var(--accent); border-radius: 3px; transition: width 600ms linear; }
  .done .fill { background: var(--ok); }
  .seed .fill { background: var(--ok); }
  .err .fill { background: var(--danger); }
  .idle .fill { background: var(--text-3); }
  .indet { position: absolute; top: 0; bottom: 0; width: 30%; background: var(--accent); border-radius: 3px; animation: indet 1.2s ease-in-out infinite; }
  @keyframes indet { from { left: -30%; } to { left: 100%; } }
  .pct { width: 42px; text-align: right; font-size: 12px; color: var(--text-2); }
  .c-status { display: flex; align-items: center; gap: 7px; font-size: 12.5px; color: var(--text-2); white-space: nowrap; }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--text-3); flex-shrink: 0; }
  .run .dot { background: var(--accent); }
  .seed .dot, .done .dot { background: var(--ok); }
  .err .dot { background: var(--danger); }
  .err .c-status { color: var(--danger); }
  .empty { height: 100%; display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center; gap: 6px; padding: 40px; }
  .empty p { margin: 0; }
  .hero { width: 64px; height: 64px; border-radius: 18px; display: grid; place-items: center; background: color-mix(in srgb, var(--accent) 16%, transparent); color: var(--accent); margin-bottom: 8px; }
  .big { font: 600 17px var(--font-display); }
  @media (max-width: 980px) { .head, .row { grid-template-columns: minmax(160px, 1fr) 76px 120px 84px 100px; } .c-eta { display: none !important; } }
</style>
