<script lang="ts">
  import { onMount } from 'svelte';
  import Modal from './Modal.svelte';
  import Icon from './Icon.svelte';
  import { api, isTauri, type HfFile, type HfRepo } from '../api';
  import { app, ui, toast, errorText } from '../state.svelte';
  import { bytes } from '../format.js';

  let { input = '' }: { input?: string } = $props();

  // svelte-ignore state_referenced_locally
  let query = $state(input);
  let repo = $state<HfRepo | null>(null);
  let files = $state<HfFile[]>([]);
  let picked = $state<boolean[]>([]);
  let loading = $state(false);
  let error = $state('');
  let busy = $state(false);
  let filter = $state('');
  let folder = $state('');

  const shown = $derived(files.map((f, i) => ({ ...f, i })).filter((f) => !filter || f.path.toLowerCase().includes(filter.toLowerCase())));
  const total = $derived(files.reduce((s, f, i) => s + (picked[i] ? f.size : 0), 0));
  const count = $derived(picked.filter(Boolean).length);
  const defaultFolder = $derived(repo ? `${app.settings?.downloadDir ?? ''}\\Hugging Face\\${repo.id.replace(/\//g, '\\')}` : '');

  async function list() {
    const q = query.trim();
    if (!q || loading) return;
    loading = true;
    error = '';
    files = [];
    repo = null;
    try {
      const r = await api.hfList(q);
      repo = r.repo;
      files = r.files;
      // A link to one file picks just that file; otherwise everything.
      picked = r.files.map((f) => !r.repo.file || f.path === r.repo.file || f.path.startsWith(r.repo.file + '/'));
    } catch (e) {
      error = errorText(e);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    if (input) void list();
  });

  function setShown(v: boolean) {
    const ids = new Set(shown.map((f) => f.i));
    picked = picked.map((p, i) => (ids.has(i) ? v : p));
  }

  /** Just the weights in one format (e.g. only .safetensors or one GGUF quant). */
  function only(ext: string) {
    picked = files.map((f) => f.path.toLowerCase().endsWith(ext) || /(^|\/)(config|tokenizer|generation_config|special_tokens_map|tokenizer_config)\.json$|\.model$|readme\.md$/i.test(f.path));
  }
  const exts = $derived([...new Set(files.map((f) => (f.path.match(/\.(safetensors|gguf|bin|onnx|pt|pth|ckpt)$/i)?.[0] ?? '').toLowerCase()).filter(Boolean))]);

  async function browse() {
    if (!isTauri) return;
    const { open } = await import('@tauri-apps/plugin-dialog');
    const p = await open({ directory: true, defaultPath: folder || app.settings?.downloadDir });
    if (typeof p === 'string') folder = p;
  }

  async function add() {
    if (!repo) return;
    busy = true;
    try {
      const sel = files.filter((_, i) => picked[i]);
      const ids = await api.hfAdd({ repo: $state.snapshot(repo) as HfRepo, files: $state.snapshot(sel) as HfFile[], folder });
      toast(`Queued ${ids.length} file${ids.length === 1 ? '' : 's'} from ${repo.id}`, 'success');
      ui.dialog = null;
    } catch (e) {
      toast(errorText(e), 'error', 8000);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title="Hugging Face" width={700} onclose={() => (ui.dialog = null)}>
  <div class="hf">
    <div class="bar">
      <!-- svelte-ignore a11y_autofocus -->
      <input class="input mono" bind:value={query} placeholder="org/model, or a huggingface.co link" spellcheck="false" autofocus onkeydown={(e) => e.key === 'Enter' && list()} />
      <button class="btn primary" disabled={loading || !query.trim()} onclick={list}>{#if loading}<span class="spinner"></span>{/if}List files</button>
    </div>

    {#if repo}
      <div class="info">
        <Icon name="box" size={16} />
        <b>{repo.id}</b>
        {#if repo.kind === 'dataset'}<span class="chip">Dataset</span>{/if}
        {#if repo.revision && repo.revision !== 'main'}<span class="chip mono">{repo.revision}</span>{/if}
        <span class="muted">{files.length} files</span>
      </div>
      <div class="tools">
        <input class="input" bind:value={filter} placeholder="Filter files" spellcheck="false" />
        {#each exts as e (e)}<button class="btn ghost" onclick={() => only(e)} title="Pick only {e} weights plus the config files">Only {e}</button>{/each}
        <button class="btn ghost" onclick={() => setShown(true)}>All</button>
        <button class="btn ghost" onclick={() => setShown(false)}>None</button>
      </div>
      <div class="list">
        {#each shown as f (f.i)}
          <label class="file" title={f.path}>
            <input type="checkbox" bind:checked={picked[f.i]} />
            <span class="p">{f.path}</span>
            <span class="v" title={f.sha256 ? 'Checked against its SHA-256 when done' : 'Checked against its git hash when done'}><Icon name="shield" size={13} /></span>
            <span class="s">{bytes(f.size)}</span>
          </label>
        {/each}
      </div>
      <label class="field">
        <span>Folder</span>
        <span class="folder"><input class="input" bind:value={folder} placeholder={defaultFolder} spellcheck="false" /><button class="btn" onclick={browse}>Browse</button></span>
      </label>
    {:else if error}
      <p class="bad"><Icon name="alert" size={15} /> {error}</p>
      {#if /token|gated|private|sign/i.test(error)}<p class="muted note">Gated and private models need a Hugging Face token: add one in Settings › Downloads.</p>{/if}
    {:else}
      <p class="muted note">Paste a model or dataset (like <span class="mono">openai/whisper-large-v3</span>). Each file downloads over several connections and is checked against its hash when it finishes.</p>
    {/if}
  </div>

  {#snippet footer()}
    {#if repo}<span class="muted sel">{count} picked · {bytes(total)}</span>{/if}
    <span class="grow"></span>
    <button class="btn" onclick={() => (ui.dialog = null)}>Cancel</button>
    <button class="btn primary" disabled={!repo || !count || busy} onclick={add}>{#if busy}<span class="spinner"></span>{/if}Download {count || ''}</button>
  {/snippet}
</Modal>

<style>
  .hf { display: flex; flex-direction: column; gap: 12px; }
  .bar, .tools { display: flex; gap: 8px; }
  .bar .input, .tools .input { flex: 1; }
  .info { display: flex; align-items: center; gap: 8px; font-size: 13px; }
  .list { height: 280px; overflow: auto; border: 1px solid var(--line); border-radius: 8px; }
  .file { display: grid; grid-template-columns: 20px 1fr 18px 80px; gap: 8px; align-items: center; height: 30px; padding: 0 10px; font-size: 12.5px; border-bottom: 1px solid var(--line); cursor: pointer; content-visibility: auto; contain-intrinsic-size: auto 30px; }
  .file:hover { background: var(--surface-hover); }
  .p { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-family: var(--mono); font-size: 12px; }
  .v { color: var(--ok); display: flex; }
  .s { text-align: right; color: var(--text-2); font-variant-numeric: tabular-nums; }
  .field { display: flex; flex-direction: column; gap: 5px; font-size: 12.5px; }
  .field > span:first-child { font-weight: 600; color: var(--text-2); }
  .folder { display: flex; gap: 6px; }
  .folder .input { flex: 1; }
  .bad { color: var(--danger); display: flex; align-items: center; gap: 6px; margin: 0; }
  .note { margin: 0; font-size: 13px; }
  .sel { font-size: 12.5px; }
  .grow { flex: 1; }
</style>
