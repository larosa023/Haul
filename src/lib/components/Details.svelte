<script lang="ts">
  import Icon from './Icon.svelte';
  import Switch from './Switch.svelte';
  import { api, type Item, type TorrentFile } from '../api';
  import { app, ui, selectedItems, toast, errorText } from '../state.svelte';
  import { bytes, date, duration, percent, statusLabel } from '../format.js';

  const sel = $derived(selectedItems());
  const it = $derived(sel.length === 1 ? sel[0] : null);
  let height = $state(Number(localStorage.getItem('haul:details-h')) || 250);

  let files: TorrentFile[] = $state([]);
  let filesFor = 0;
  let filesTimer: ReturnType<typeof setInterval> | undefined;

  async function loadFiles(id: number) {
    try {
      files = await api.files(id);
    } catch {
      files = [];
    }
  }

  // Files refresh while the tab is open.
  $effect(() => {
    const id = it?.kind === 'torrent' && ui.detailsTab === 'files' ? it.id : 0;
    clearInterval(filesTimer);
    if (!id) return;
    if (filesFor !== id) files = [];
    filesFor = id;
    void loadFiles(id);
    filesTimer = setInterval(() => loadFiles(id), 2000);
    return () => clearInterval(filesTimer);
  });

  async function setPrio(f: TorrentFile, p: number) {
    if (!it) return;
    const pr = files.map((x) => (x.index === f.index ? p : x.priority));
    files = files.map((x) => (x.index === f.index ? { ...x, priority: p } : x));
    try {
      await api.setFilePriorities(it.id, pr);
    } catch (e) {
      toast(errorText(e), 'error');
      void loadFiles(it.id);
    }
  }

  async function setAll(p: number) {
    if (!it) return;
    const pr = files.map(() => p);
    files = files.map((x) => ({ ...x, priority: p }));
    await api.setFilePriorities(it.id, pr).catch((e) => toast(errorText(e), 'error'));
  }

  function setOpt(options: Record<string, unknown>) {
    void api.setOptions(sel.map((i) => i.id), options).catch((e) => toast(errorText(e), 'error'));
  }

  function drag(e: PointerEvent) {
    const startY = e.clientY;
    const startH = height;
    const move = (ev: PointerEvent) => (height = Math.max(140, Math.min(window.innerHeight - 220, startH - (ev.clientY - startY))));
    const up = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      localStorage.setItem('haul:details-h', String(Math.round(height)));
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  }

  function copy(text: string) {
    void navigator.clipboard.writeText(text).then(() => toast('Copied', 'success', 1500));
  }

  function ratio(i: Item) {
    return i.size ? (i.uploaded / i.size).toFixed(2) : '0.00';
  }

  const tabs = $derived(it?.kind === 'torrent' ? ['general', 'files', 'options'] : ['general', 'options']);
  $effect(() => {
    if (!tabs.includes(ui.detailsTab)) ui.detailsTab = 'general';
  });
</script>

<section class="details" style="height: {height}px">
  <div class="grip" role="separator" aria-orientation="horizontal" onpointerdown={drag}></div>
  {#if !sel.length}
    <div class="none muted">Select a download to see its details.</div>
  {:else}
    <div class="tabs">
      {#if it}
        {#each tabs as t (t)}
          <button class:on={ui.detailsTab === t} onclick={() => (ui.detailsTab = t)}>{t === 'general' ? 'General' : t === 'files' ? `Files (${it.fileCount})` : 'Options'}</button>
        {/each}
      {:else}
        <button class="on">Options for {sel.length} downloads</button>
      {/if}
      <span class="grow"></span>
      <button class="icon-btn" title="Close details (F2)" onclick={() => (ui.details = false)}><Icon name="x" size={15} /></button>
    </div>

    <div class="body">
      {#if it && ui.detailsTab === 'general'}
        <div class="grid">
          <span class="k">Status</span><span class="v">{statusLabel(it)}{it.note ? ` · ${it.note}` : ''}</span>
          {#if it.error}<span class="k">Problem</span><span class="v err">{it.error}</span>{/if}
          <span class="k">Saved to</span>
          <span class="v path">
            <span title={it.saveDir}>{it.saveDir || 'Picked when it starts'}{it.fileName && it.kind === 'http' ? `\\${it.fileName}` : ''}</span>
            {#if it.saveDir}<button class="btn sm ghost" onclick={() => api.reveal(it.id)}><Icon name="folder" size={14} />Show</button>{/if}
          </span>
          <span class="k">Size</span><span class="v">{it.size ? bytes(it.size) : 'Unknown'} · {bytes(it.done)} done ({percent(it.done, it.size).toFixed(1)}%)</span>
          {#if it.kind === 'torrent'}
            <span class="k">Shared</span><span class="v">{bytes(it.uploaded)} · ratio {ratio(it)}{it.seedingSecs ? ` · seeded ${duration(it.seedingSecs)}` : ''}</span>
            <span class="k">Peers</span><span class="v">{it.peers}</span>
            <span class="k">Info hash</span><span class="v mono copyable"><span>{it.infoHash}</span><button class="icon-btn small" title="Copy magnet link" onclick={() => copy(`magnet:?xt=urn:btih:${it.infoHash}&dn=${encodeURIComponent(it.name)}`)}><Icon name="copy" size={13} /></button></span>
          {:else}
            <span class="k">Link</span><span class="v copyable"><span class="url" title={it.source}>{it.source}</span><button class="icon-btn small" title="Copy link" onclick={() => copy(it.source)}><Icon name="copy" size={13} /></button></span>
            <span class="k">Connections</span><span class="v">{it.resumable ? `${it.segments || '—'} parts · resumable` : it.size ? 'Single connection (server can’t resume)' : '—'}</span>
            {#if it.sha256}<span class="k">SHA-256</span><span class="v mono">{it.sha256} {it.hashState === 'ok' ? '✓ verified' : it.hashState === 'mismatch' ? '✗ mismatch' : it.hashState === 'checking' ? '(checking…)' : '(checked when done)'}</span>{/if}
          {/if}
          <span class="k">Category</span><span class="v">{it.category || '—'}</span>
          <span class="k">Added</span><span class="v">{date(it.added)}</span>
          {#if it.finishedAt}<span class="k">Finished</span><span class="v">{date(it.finishedAt)}</span>{/if}
        </div>
      {:else if it && ui.detailsTab === 'files'}
        <div class="files">
          <div class="fhead">
            <span>{files.length} files</span>
            <span class="grow"></span>
            <button class="btn sm ghost" onclick={() => setAll(2)}>Download all</button>
          </div>
          {#each files as f (f.index)}
            <div class="file" class:skip={f.priority === 0}>
              <input type="checkbox" checked={f.priority > 0} onchange={(e) => setPrio(f, (e.currentTarget as HTMLInputElement).checked ? 2 : 0)} aria-label="Download {f.path}" />
              <span class="fname" title={f.path}>{f.path}</span>
              <span class="fsize">{bytes(f.size)}</span>
              <span class="fbar"><span style="width: {percent(f.done, f.size)}%"></span></span>
              <select class="select fsel" value={String(f.priority)} onchange={(e) => setPrio(f, Number((e.currentTarget as HTMLSelectElement).value))} aria-label="Priority">
                <option value="0">Skip</option>
                <option value="1">Low</option>
                <option value="2">Normal</option>
                <option value="3">High</option>
              </select>
            </div>
          {:else}
            <div class="muted pad">Loading…</div>
          {/each}
        </div>
      {:else}
        {@const one = sel[0]}
        {@const torrents = sel.every((i) => i.kind === 'torrent')}
        {@const direct = sel.every((i) => i.kind === 'http')}
        <div class="opts">
          <label class="opt">
            <span>Download limit</span>
            <span class="num"><input class="input" type="number" min="0" value={one.limitDown} onchange={(e) => setOpt({ limitDown: Number((e.currentTarget as HTMLInputElement).value) || 0 })} /> KB/s</span>
            <small class="faint">0 = only the global limit</small>
          </label>
          {#if torrents}
            <label class="opt">
              <span>Upload limit</span>
              <span class="num"><input class="input" type="number" min="0" value={one.limitUp} onchange={(e) => setOpt({ limitUp: Number((e.currentTarget as HTMLInputElement).value) || 0 })} /> KB/s</span>
              <small class="faint">0 = only the global limit</small>
            </label>
          {/if}
          <div class="opt">
            <span>Queue priority</span>
            <div class="seg">
              {#each [[-1, 'Low'], [0, 'Normal'], [1, 'High']] as [v, l] (v)}
                <button class:on={one.priority === v} onclick={() => setOpt({ priority: v })}>{l}</button>
              {/each}
            </div>
          </div>
          {#if torrents}
            <div class="opt">
              <span>Download in order</span>
              <Switch checked={one.sequential} label="Sequential download" onchange={(v) => setOpt({ sequential: v })} />
              <small class="faint">Lets you preview videos while they download</small>
            </div>
            <label class="opt">
              <span>Stop seeding at ratio</span>
              <span class="num">
                <select class="select" value={one.ratioLimit < 0 ? 'global' : one.ratioLimit === 0 ? 'none' : 'custom'}
                  onchange={(e) => { const v = (e.currentTarget as HTMLSelectElement).value; setOpt({ ratioLimit: v === 'global' ? -1 : v === 'none' ? 0 : 2 }); }}>
                  <option value="global">Use Settings ({app.settings?.ratioLimit || 'no limit'})</option>
                  <option value="none">No limit</option>
                  <option value="custom">Custom</option>
                </select>
                {#if one.ratioLimit > 0}<input class="input short" type="number" min="0.1" step="0.1" value={one.ratioLimit} onchange={(e) => setOpt({ ratioLimit: Math.max(0.1, Number((e.currentTarget as HTMLInputElement).value) || 1) })} />{/if}
              </span>
            </label>
            <label class="opt">
              <span>Stop seeding after</span>
              <span class="num">
                <select class="select" value={one.seedTimeLimit < 0 ? 'global' : one.seedTimeLimit === 0 ? 'none' : 'custom'}
                  onchange={(e) => { const v = (e.currentTarget as HTMLSelectElement).value; setOpt({ seedTimeLimit: v === 'global' ? -1 : v === 'none' ? 0 : 60 }); }}>
                  <option value="global">Use Settings ({app.settings?.seedTimeLimit ? `${app.settings.seedTimeLimit} min` : 'no limit'})</option>
                  <option value="none">No limit</option>
                  <option value="custom">Custom</option>
                </select>
                {#if one.seedTimeLimit > 0}<input class="input short" type="number" min="1" value={one.seedTimeLimit} onchange={(e) => setOpt({ seedTimeLimit: Math.max(1, Number((e.currentTarget as HTMLInputElement).value) || 60) })} /> min{/if}
              </span>
            </label>
          {/if}
          {#if direct}
            <label class="opt">
              <span>Connections</span>
              <span class="num"><input class="input" type="number" min="0" max="32" value={one.connections} onchange={(e) => setOpt({ connections: Number((e.currentTarget as HTMLInputElement).value) || 0 })} /></span>
              <small class="faint">0 = default ({app.settings?.connections}). Applies when it next starts.</small>
            </label>
            {#if it}
              <label class="opt wide">
                <span>Expected SHA-256</span>
                <input class="input mono" value={it.sha256} placeholder="Paste a SHA-256 to verify the file" spellcheck="false" onchange={(e) => setOpt({ sha256: (e.currentTarget as HTMLInputElement).value })} />
              </label>
            {/if}
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</section>

<style>
  .details { position: relative; flex-shrink: 0; display: flex; flex-direction: column; margin: 0 12px 12px; border: 1px solid var(--line); border-radius: 0 0 8px 8px; background: var(--surface); min-height: 0; }
  .grip { position: absolute; top: -4px; left: 0; right: 0; height: 8px; cursor: ns-resize; z-index: 2; }
  .grip:hover { background: linear-gradient(transparent 3px, var(--accent) 3px, var(--accent) 5px, transparent 5px); }
  .none { margin: auto; }
  .tabs { display: flex; align-items: center; gap: 2px; padding: 4px 6px 0 8px; border-bottom: 1px solid var(--line); }
  .tabs button:not(.icon-btn) { border: 0; background: none; padding: 8px 12px; cursor: pointer; color: var(--text-2); font-weight: 600; font-size: 12.5px; border-bottom: 2px solid transparent; }
  .tabs button.on { color: var(--text); border-bottom-color: var(--accent); }
  .tabs .icon-btn { width: 28px; height: 28px; }
  .grow { flex: 1; }
  .body { flex: 1; overflow: auto; padding: 12px 16px; user-select: text; }
  .grid { display: grid; grid-template-columns: 110px 1fr; gap: 7px 14px; font-size: 12.5px; align-items: baseline; }
  .k { color: var(--text-3); }
  .v { min-width: 0; overflow-wrap: anywhere; }
  .err { color: var(--danger); }
  .path { display: flex; align-items: center; gap: 8px; }
  .path > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .copyable { display: flex; align-items: center; gap: 6px; }
  .url { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .icon-btn.small { width: 22px; height: 22px; }
  .files { display: flex; flex-direction: column; font-size: 12.5px; user-select: none; }
  .fhead { display: flex; align-items: center; color: var(--text-3); padding-bottom: 6px; }
  .file { display: grid; grid-template-columns: 18px 1fr 72px 90px 96px; gap: 10px; align-items: center; padding: 4px 0; border-top: 1px solid var(--line); }
  .file.skip .fname, .file.skip .fsize { color: var(--text-3); text-decoration: line-through; }
  .fname { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .fsize { text-align: right; color: var(--text-2); font-variant-numeric: tabular-nums; }
  .fbar { height: 4px; border-radius: 2px; background: var(--surface-strong); overflow: hidden; }
  .fbar span { display: block; height: 100%; background: var(--accent); }
  .fsel { height: 26px; font-size: 12px; }
  .pad { padding: 10px 0; }
  .opts { display: grid; grid-template-columns: repeat(auto-fill, minmax(250px, 1fr)); gap: 14px 24px; }
  .opt { display: flex; flex-direction: column; gap: 6px; font-size: 12.5px; }
  .opt > span:first-child { font-weight: 600; }
  .opt.wide { grid-column: 1 / -1; }
  .num { display: flex; align-items: center; gap: 8px; color: var(--text-2); }
  .num .input { width: 100px; }
  .num .input.short { width: 80px; }
  .num .select { width: auto; }
</style>
