<script lang="ts">
  import Icon from './Icon.svelte';
  import { api } from '../api';
  import { app, ui, toast, errorText } from '../state.svelte';
  import { urlName, linkKind } from '../format.js';

  const u = $derived(app.update);
  const showUpdate = $derived(!!u && !ui.updateDismissed && (u.status === 'ready' || u.status === 'applying' || (u.status === 'downloading' && u.progress > 0)));
  const busy = $derived((app.totals?.downloading ?? 0) > 0);

  async function restart(force: boolean) {
    try {
      await api.applyUpdate(force);
      if (!force) toast('Haul will restart to update when downloads finish', 'info');
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }

  function grab() {
    const url = ui.clipboard;
    ui.clipboard = '';
    ui.dialog = { kind: 'add', url };
  }
</script>

{#if showUpdate && u}
  <div class="banner" role="status">
    <span class="dot"></span>
    {#if u.status === 'downloading'}
      <span>Downloading Haul {u.version}… {Math.round(u.progress * 100)}%</span>
    {:else if u.status === 'applying'}
      <span class="spinner"></span><span>Restarting into Haul {u.version}…</span>
    {:else}
      <span><strong>Update ready, restart</strong> <span class="muted">to get Haul {u.version}.{u.waiting && busy ? ' It will restart by itself when downloads finish.' : ''} Downloads pick up where they left off.</span></span>
      <span class="grow"></span>
      {#if busy && !u.waiting}<button class="btn sm" onclick={() => restart(false)}>When downloads finish</button>{/if}
      <button class="btn sm primary" onclick={() => restart(true)}>Restart now</button>
      <button class="icon-btn" aria-label="Later" title="Later" onclick={() => (ui.updateDismissed = true)}><Icon name="x" size={15} /></button>
    {/if}
  </div>
{/if}

{#if ui.clipboard}
  <div class="banner clip" role="status">
    <Icon name={linkKind(ui.clipboard) === 'torrent' ? 'magnet' : 'link'} size={16} />
    <span class="txt">Download <strong>{urlName(ui.clipboard)}</strong> from your clipboard?</span>
    <span class="grow"></span>
    <button class="btn sm primary" onclick={grab}>Download</button>
    <button class="icon-btn" aria-label="Dismiss" title="Dismiss" onclick={() => (ui.clipboard = '')}><Icon name="x" size={15} /></button>
  </div>
{/if}

<style>
  .banner {
    display: flex; align-items: center; gap: 10px; margin: 0 12px 8px; padding: 6px 8px 6px 14px; min-height: 42px; border-radius: 8px; font-size: 13px;
    background: color-mix(in srgb, var(--accent) 13%, var(--surface)); border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent);
  }
  .clip { background: var(--surface); border-color: var(--line); }
  .txt { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--accent); flex-shrink: 0; }
  .grow { flex: 1; }
</style>
