<script lang="ts">
  import Modal from './Modal.svelte';
  import { api } from '../api';
  import { app, ui, toast, errorText } from '../state.svelte';

  let { ids }: { ids: number[] } = $props();
  let deleteFiles = $state(false);
  let busy = $state(false);
  const names = $derived(ids.map((id) => app.items[id]?.name).filter(Boolean));

  async function go() {
    busy = true;
    try {
      await api.remove(ids, deleteFiles);
      ui.dialog = null;
    } catch (e) {
      toast(errorText(e), 'error');
    } finally {
      busy = false;
    }
  }
</script>

<Modal title={ids.length === 1 ? 'Remove download?' : `Remove ${ids.length} downloads?`} width={460} onclose={() => (ui.dialog = null)}>
  <p class="who">{names.length === 1 ? names[0] : `${names.slice(0, 3).join(', ')}${names.length > 3 ? ` and ${names.length - 3} more` : ''}`}</p>
  <label class="check">
    <input type="checkbox" bind:checked={deleteFiles} />
    <span>Also delete the downloaded files</span>
  </label>
  <p class="muted small">{deleteFiles ? 'Files are deleted permanently (not to the Recycle Bin).' : 'Files already downloaded stay where they are. Unfinished parts are cleaned up.'}</p>
  {#snippet footer()}
    <button class="btn" onclick={() => (ui.dialog = null)}>Cancel</button>
    <button class="btn primary" class:danger={deleteFiles} disabled={busy} onclick={go}>{deleteFiles ? 'Remove and delete' : 'Remove'}</button>
  {/snippet}
</Modal>

<style>
  .who { margin: 0 0 14px; font-weight: 600; overflow-wrap: anywhere; }
  .check { display: flex; align-items: center; gap: 9px; cursor: pointer; }
  .small { font-size: 12.5px; margin: 8px 0 0; }
</style>
