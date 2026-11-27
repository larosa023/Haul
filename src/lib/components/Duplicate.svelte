<script lang="ts">
  // "You already have this": shown in the Add and video windows.
  import Icon from './Icon.svelte';
  import { api, type Item } from '../api';
  import { ui, selectOnly } from '../state.svelte';
  import { bytes, date } from '../format.js';

  let { item }: { item: Item } = $props();
  const done = $derived(item.status === 'completed' || item.status === 'seeding');
  const what = $derived(
    done
      ? `You already downloaded this${item.finishedAt ? ` on ${date(item.finishedAt)}` : ''}`
      : item.status === 'error'
        ? 'This is already in your list (it stopped with an error)'
        : 'This is already in your list',
  );

  function show() {
    ui.dialog = null;
    ui.view = 'list';
    selectOnly(item.id);
  }
</script>

<div class="dup" role="status">
  <Icon name="copy" size={16} />
  <div class="txt">
    <b>{what}</b>
    <span class="muted">{item.name}{item.size ? ` · ${bytes(item.size)}` : ''}</span>
  </div>
  {#if done}<button class="btn sm" onclick={() => api.open(item.id)}>Open</button>{/if}
  <button class="btn sm ghost" onclick={show}>Show it</button>
</div>

<style>
  .dup { display: flex; align-items: center; gap: 10px; padding: 9px 12px; border-radius: 9px; border: 1px solid color-mix(in srgb, var(--warn) 45%, var(--line)); background: color-mix(in srgb, var(--warn) 9%, transparent); color: var(--text); font-size: 12.5px; }
  .dup :global(svg) { color: var(--warn); flex-shrink: 0; }
  .txt { flex: 1; display: flex; flex-direction: column; min-width: 0; }
  .txt span { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
</style>
