<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import { api } from '../api';
  import { ui, selection, selectedItems, refreshOrder, toast, errorText } from '../state.svelte';

  let pos: { x: number; y: number } | null = $state(null);
  let el: HTMLDivElement | undefined = $state();
  const sel = $derived(selectedItems());
  const one = $derived(sel.length === 1 ? sel[0] : null);

  onMount(() => {
    const open = (e: Event) => (pos = (e as CustomEvent<{ x: number; y: number }>).detail);
    const close = () => (pos = null);
    window.addEventListener('haul:menu', open);
    window.addEventListener('blur', close);
    return () => {
      window.removeEventListener('haul:menu', open);
      window.removeEventListener('blur', close);
    };
  });

  $effect(() => {
    if (pos && el) {
      const r = el.getBoundingClientRect();
      if (r.right > innerWidth) pos.x = Math.max(4, innerWidth - r.width - 4);
      if (r.bottom > innerHeight) pos.y = Math.max(4, innerHeight - r.height - 4);
      el.querySelector<HTMLElement>('button')?.focus();
    }
  });

  async function act(fn: () => Promise<unknown> | void) {
    pos = null;
    try {
      await fn();
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }

  const ids = () => [...selection];
  function key(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      pos = null;
    }
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const b = [...(el?.querySelectorAll<HTMLElement>('button:not(:disabled)') ?? [])];
      const i = b.indexOf(document.activeElement as HTMLElement);
      b[(i + (e.key === 'ArrowDown' ? 1 : b.length - 1)) % b.length]?.focus();
    }
  }
</script>

{#if pos}
  <div class="scrim" role="presentation" onmousedown={() => (pos = null)} oncontextmenu={(e) => { e.preventDefault(); pos = null; }}></div>
  <div class="menu" role="menu" tabindex="-1" bind:this={el} style="left: {pos.x}px; top: {pos.y}px" onkeydown={key}>
    {#if one}
      <button role="menuitem" disabled={!(one.status === 'completed' || one.status === 'seeding')} onclick={() => act(() => api.open(one.id))}><Icon name="open" size={15} />Open</button>
      <button role="menuitem" onclick={() => act(() => api.reveal(one.id))}><Icon name="folder" size={15} />Show in folder</button>
      <hr />
    {/if}
    <button role="menuitem" onclick={() => act(() => api.resume(ids()))}><Icon name="play" size={15} />Resume</button>
    <button role="menuitem" onclick={() => act(() => api.pause(ids()))}><Icon name="pause" size={15} />Pause</button>
    {#if sel.some((i) => i.status === 'error' || i.hashState === 'mismatch')}
      <button role="menuitem" onclick={() => act(() => api.retry(ids()))}><Icon name="refresh" size={15} />Retry</button>
    {/if}
    <hr />
    <button role="menuitem" onclick={() => act(async () => refreshOrder(await api.move(ids(), 'top')))}><Icon name="top" size={15} />Move to top</button>
    <button role="menuitem" onclick={() => act(async () => refreshOrder(await api.move(ids(), 'bottom')))}><Icon name="bottom" size={15} />Move to bottom</button>
    <button role="menuitem" onclick={() => act(() => { ui.details = true; ui.detailsTab = 'options'; })}><Icon name="gauge" size={15} />Speed limit and options…</button>
    {#if one}
      <button role="menuitem" onclick={() => act(() => navigator.clipboard.writeText(one.kind === 'torrent' ? `magnet:?xt=urn:btih:${one.infoHash}&dn=${encodeURIComponent(one.name)}` : one.source))}><Icon name="copy" size={15} />Copy {one.kind === 'torrent' ? 'magnet link' : 'link'}</button>
    {/if}
    <hr />
    <button role="menuitem" class="danger" onclick={() => act(() => { ui.dialog = { kind: 'remove', ids: ids() }; })}><Icon name="trash" size={15} />Remove…</button>
  </div>
{/if}

<style>
  .scrim { position: fixed; inset: 0; z-index: 60; }
  .menu {
    position: fixed; z-index: 61; min-width: 220px; padding: 5px; border-radius: 8px; display: flex; flex-direction: column;
    background: var(--solid-bg, #1d1d22); border: 1px solid var(--line); box-shadow: 0 14px 40px rgb(0 0 0 / 0.4); outline: none;
  }
  button { display: flex; align-items: center; gap: 10px; height: 32px; padding: 0 10px; border: 0; border-radius: 5px; background: none; cursor: pointer; text-align: left; color: var(--text); }
  button:hover, button:focus-visible { background: var(--surface-hover); outline: none; }
  button:disabled { color: var(--text-3); cursor: default; background: none; }
  button :global(svg) { color: var(--text-2); }
  .danger { color: var(--danger); }
  .danger :global(svg) { color: var(--danger); }
  hr { border: 0; border-top: 1px solid var(--line); margin: 4px 2px; }
</style>
