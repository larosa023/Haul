<script lang="ts">
  import Modal from './Modal.svelte';
  import Switch from './Switch.svelte';
  import { api, isTauri, type TorrentMeta } from '../api';
  import { app, ui, toast, errorText, selectOnly } from '../state.svelte';
  import { bytes } from '../format.js';

  let { meta }: { meta: TorrentMeta } = $props();

  // svelte-ignore state_referenced_locally
  let prio: number[] = $state(meta.files.map(() => 2));
  // svelte-ignore state_referenced_locally
  let folder = $state(meta.folder);
  // svelte-ignore state_referenced_locally
  let category = $state(meta.category);
  let paused = $state(false);
  let sequential = $state(false);
  let filter = $state('');
  let busy = $state(false);
  let folderEdited = false;

  const categories = $derived(Object.keys(app.settings?.folders ?? {}));
  const selectedSize = $derived(meta.files.reduce((s, f, i) => s + (prio[i] > 0 ? f[1] : 0), 0));
  const selectedCount = $derived(prio.filter((p) => p > 0).length);
  const shown = $derived(
    meta.files.map((f, i) => ({ path: f[0], size: f[1], i })).filter((f) => !filter || f.path.toLowerCase().includes(filter.toLowerCase())),
  );
  const subfolder = $derived(meta.files.length > 1 ? `\\${meta.name.replace(/[<>:"/\\|?*]/g, '_')}` : '');

  function onCategory(c: string) {
    category = c;
    if (!folderEdited && app.settings) folder = app.settings.sortEnabled ? (app.settings.folders[c] ?? app.settings.downloadDir) : app.settings.downloadDir;
  }

  function setShown(p: number) {
    const ids = new Set(shown.map((f) => f.i));
    prio = prio.map((v, i) => (ids.has(i) ? p : v));
  }

  async function browse() {
    if (!isTauri) return;
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({ directory: true, defaultPath: folder });
    if (typeof picked === 'string') {
      folder = picked;
      folderEdited = true;
    }
  }

  async function add() {
    busy = true;
    try {
      await api.confirmTorrent({ id: meta.id, folder, priorities: $state.snapshot(prio), paused, sequential, category });
      ui.dialog = null;
      selectOnly(meta.id);
    } catch (e) {
      toast(errorText(e), 'error');
    } finally {
      busy = false;
    }
  }

  async function cancel() {
    ui.dialog = null;
    await api.cancelTorrent(meta.id).catch(() => {});
  }
</script>

<Modal title={meta.name} width={720} onclose={cancel}>
  <div class="top">
    <div class="row2">
      <label class="field">
        <span>Category</span>
        <select class="select" value={category} onchange={(e) => onCategory((e.currentTarget as HTMLSelectElement).value)}>
          {#each categories as c (c)}<option value={c}>{c}</option>{/each}
        </select>
      </label>
      <label class="field">
        <span>Save in</span>
        <span class="folder">
          <input class="input" bind:value={folder} oninput={() => (folderEdited = true)} spellcheck="false" />
          <button class="btn" onclick={browse}>Browse</button>
        </span>
        {#if subfolder}<small class="faint">Files go in {folder}{subfolder}</small>{/if}
      </label>
    </div>
    {#if meta.debrid}
      <p class="muted rdnote">Real-Debrid downloads the files you pick, then Haul fetches them at full speed into this folder.</p>
    {:else}
      <div class="flags">
        <label class="flag"><Switch bind:checked={sequential} label="Download in order" /> Download in order</label>
        <label class="flag"><Switch bind:checked={paused} label="Add paused" /> Add paused</label>
      </div>
    {/if}
  </div>

  <div class="fbar">
    <strong>{selectedCount} of {meta.files.length} files · {bytes(selectedSize)}</strong>
    <span class="grow"></span>
    {#if meta.files.length > 8}<input class="input find" bind:value={filter} placeholder="Filter files" spellcheck="false" />{/if}
    <button class="btn sm ghost" onclick={() => setShown(2)}>All</button>
    <button class="btn sm ghost" onclick={() => setShown(0)}>None</button>
  </div>
  <div class="files">
    {#each shown.slice(0, 2000) as f (f.i)}
      <div class="file" class:skip={prio[f.i] === 0}>
        <input type="checkbox" checked={prio[f.i] > 0} onchange={(e) => (prio[f.i] = (e.currentTarget as HTMLInputElement).checked ? 2 : 0)} aria-label={f.path} />
        <span class="name" title={f.path}>{f.path}</span>
        <span class="size">{bytes(f.size)}</span>
        {#if !meta.debrid}<select class="select sel" value={String(prio[f.i])} onchange={(e) => (prio[f.i] = Number((e.currentTarget as HTMLSelectElement).value))} aria-label="Priority">
          <option value="0">Skip</option>
          <option value="1">Low</option>
          <option value="2">Normal</option>
          <option value="3">High</option>
        </select>{/if}
      </div>
    {/each}
    {#if shown.length > 2000}<div class="muted more">…and {shown.length - 2000} more (use the filter)</div>{/if}
  </div>

  {#snippet footer()}
    <span class="muted hash mono" title="Info hash">{meta.hash}</span>
    <span class="grow"></span>
    <button class="btn" onclick={cancel}>Cancel</button>
    <button class="btn primary" disabled={busy || !selectedCount} onclick={add}>{#if busy}<span class="spinner"></span>{/if}{meta.debrid ? 'Send to Real-Debrid' : 'Add torrent'}</button>
  {/snippet}
</Modal>

<style>
  .top { display: flex; flex-direction: column; gap: 12px; margin-bottom: 14px; }
  .row2 { display: grid; grid-template-columns: 150px 1fr; gap: 12px; }
  .field { display: flex; flex-direction: column; gap: 5px; font-size: 12.5px; }
  .field > span:first-child { font-weight: 600; color: var(--text-2); }
  .folder { display: flex; gap: 6px; }
  .flags { display: flex; gap: 22px; }
  .flag { display: flex; align-items: center; gap: 9px; font-size: 13px; cursor: pointer; }
  .fbar { display: flex; align-items: center; gap: 6px; padding: 6px 0; font-size: 12.5px; border-bottom: 1px solid var(--line); }
  .find { width: 180px; height: 28px; }
  .grow { flex: 1; }
  .files { max-height: 42vh; overflow: auto; }
  .rdnote { margin: 0; font-size: 12.5px; }
  .file { display: grid; grid-template-columns: 18px 1fr 80px 92px; gap: 10px; align-items: center; padding: 4px 0; font-size: 12.5px; border-bottom: 1px solid var(--line); }
  .file.skip .name, .file.skip .size { color: var(--text-3); }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .size { text-align: right; color: var(--text-2); font-variant-numeric: tabular-nums; }
  .sel { height: 26px; font-size: 12px; }
  .more { padding: 8px 0; font-size: 12px; }
  .hash { font-size: 11px; overflow: hidden; text-overflow: ellipsis; }
</style>
