<script lang="ts">
  import { debridName } from '../format.js';
  import Icon from './Icon.svelte';
  import Switch from './Switch.svelte';
  import SpeedGraph from './SpeedGraph.svelte';
  import SegmentMap from './SegmentMap.svelte';
  import Safety from './Safety.svelte';
  import { api, type Item, type TorrentFile } from '../api';
  import { app, ui, history, selectedItems, toast, errorText } from '../state.svelte';
  import { bytes, date, duration, percent, statusLabel, speed, eta } from '../format.js';

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

  // Live view: speed graph for anything downloading, and the parts map for direct downloads.
  const live = $derived(!!it && ui.detailsTab === 'general' && (it.status === 'downloading' || !!history.items[it.id]?.length));
  const hist = $derived(it ? (history.items[it.id] ?? []) : []);
  const peak = $derived(hist.length ? Math.max(...hist) : 0);
  const avg = $derived(hist.length ? hist.reduce((a, b) => a + b, 0) / hist.length : 0);
  let segs: { start: number; end: number; done: number }[] = $state([]);
  $effect(() => {
    const id = live && it && (it.kind === 'http') && it.resumable ? it.id : 0;
    segs = [];
    if (!id) return;
    let alive = true;
    const load = async () => {
      const v = await api.item(id).catch(() => null);
      if (alive) segs = ((v?.segmentList as typeof segs) ?? []).slice();
    };
    void load();
    const t = setInterval(load, 1000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  });

  let scanning = $state(false);
  async function rescan(id: number) {
    scanning = true;
    try {
      await api.rescan(id);
    } catch (e) {
      toast(errorText(e), 'error');
    } finally {
      scanning = false;
    }
  }

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

  const tabs = $derived(it?.kind === 'torrent' ? ['general', 'files', 'options'] : it?.kind === 'debrid' ? ['general'] : ['general', 'options']);
  $effect(() => {
    if (!tabs.includes(ui.detailsTab)) ui.detailsTab = 'general';
  });
  const dname = $derived(debridName(app.settings?.debridService));
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
        <div class="gen" class:live>
        <div class="col">
        {#if it.safety}
          <div class="safe"><Safety report={it.safety} open={it.safety.grade >= 'C'} {scanning} onrescan={it.status === 'completed' || it.status === 'seeding' ? () => rescan(it.id) : undefined} /></div>
        {:else if it.status === 'completed' && it.kind !== 'debrid'}
          <button class="btn sm ghost safe" disabled={scanning} onclick={() => rescan(it.id)}>{#if scanning}<span class="spinner"></span>{:else}<Icon name="shield" size={14} />{/if}Check safety</button>
        {/if}
        <div class="grid">
          <span class="k">Status</span><span class="v">{statusLabel(it)}{it.note ? ` · ${it.note}` : ''}</span>
          {#if it.error}<span class="k">Problem</span><span class="v err">{it.error}</span>{/if}
          <span class="k">Saved to</span>
          <span class="v path">
            <span title={it.saveDir}>{it.saveDir || 'Picked when it starts'}{it.fileName && (it.kind === 'http' || it.kind === 'media') ? `\\${it.fileName}` : ''}</span>
            {#if it.saveDir}<button class="btn sm ghost" onclick={() => api.reveal(it.id)}><Icon name="folder" size={14} />Show</button>{/if}
          </span>
          <span class="k">Size</span><span class="v">{it.size ? bytes(it.size) : 'Unknown'} · {bytes(it.done)} done ({percent(it.done, it.size).toFixed(1)}%)</span>
          {#if it.kind === 'torrent'}
            <span class="k">Shared</span><span class="v">{bytes(it.uploaded)} · ratio {ratio(it)}{it.seedingSecs ? ` · seeded ${duration(it.seedingSecs)}` : ''}</span>
            <span class="k">Peers</span><span class="v">{it.peers}</span>
            <span class="k">Info hash</span><span class="v mono copyable"><span>{it.infoHash}</span><button class="icon-btn small" title="Copy magnet link" onclick={() => copy(`magnet:?xt=urn:btih:${it.infoHash}&dn=${encodeURIComponent(it.name)}`)}><Icon name="copy" size={13} /></button></span>
          {:else if it.kind === 'media'}
            <span class="k">Link</span><span class="v copyable"><span class="url" title={it.source}>{it.source}</span><button class="icon-btn small" title="Copy link" onclick={() => copy(it.source)}><Icon name="copy" size={13} /></button></span>
            <span class="k">Format</span><span class="v">{it.media || '—'} · downloaded with yt-dlp{it.size ? '' : ' (size known once it starts)'}</span>
          {:else if it.kind === 'debrid'}
            <span class="k">{dname}</span><span class="v">{it.note || `Waiting for ${dname}`}. When it’s done, the files download here at full speed.</span>
          {:else}
            <span class="k">Link</span><span class="v copyable"><span class="url" title={it.source}>{it.source}</span><button class="icon-btn small" title="Copy link" onclick={() => copy(it.source)}><Icon name="copy" size={13} /></button></span>
            {#if it.debrid}<span class="k">Through</span><span class="v">{dname} (the link is renewed automatically if it expires)</span>{/if}
            <span class="k">Connections</span><span class="v">{it.resumable ? `${it.segments || '—'} parts · resumable` : it.size ? 'Single connection (server can’t resume)' : '—'}</span>
            {#if it.sha256}<span class="k">SHA-256</span><span class="v mono">{it.sha256} {it.hashState === 'ok' ? '✓ verified' : it.hashState === 'mismatch' ? '✗ mismatch' : it.hashState === 'checking' ? '(checking…)' : '(checked when done)'}</span>{/if}
          {/if}
          <span class="k">Category</span><span class="v">{it.category || '—'}</span>
          <span class="k">Added</span><span class="v">{date(it.added)}</span>
          {#if it.finishedAt}<span class="k">Finished</span><span class="v">{date(it.finishedAt)}</span>{/if}
        </div>
        </div>
        {#if live && it}
          <div class="livecard">
            <div class="lhead">
              <span class="big">{speed(it.speedDown) || '—'}</span>
              <span class="muted lsub">{eta(it.eta) ? `${eta(it.eta)} left` : ''}</span>
            </div>
            <div class="gwrap"><SpeedGraph data={hist} height={70} /></div>
            <div class="stats">
              <span>Peak <b>{speed(peak) || '—'}</b></span>
              <span>Average <b>{speed(avg) || '—'}</b></span>
              <span>{it.kind === 'torrent' ? `${it.peers} peers` : it.note || ''}</span>
            </div>
            {#if segs.length}
              <SegmentMap segments={segs} size={it.size} />
              <div class="faint maplbl">{segs.filter((x) => x.done < x.end - x.start).length} of {segs.length} parts still downloading</div>
            {/if}
          </div>
        {/if}
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
              <small class="faint">0 = default ({app.settings?.connections ? app.settings.connections : 'Auto, by file size'}). Applies when it next starts.</small>
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
  .gen.live { display: grid; grid-template-columns: minmax(0, 1fr) minmax(300px, 40%); gap: 18px; align-items: start; }
  .livecard { display: flex; flex-direction: column; gap: 8px; padding: 12px 14px; border-radius: 10px; border: 1px solid var(--line); background: linear-gradient(160deg, color-mix(in srgb, var(--accent) 10%, var(--bg)), var(--bg)); user-select: none; }
  .lhead { display: flex; align-items: baseline; justify-content: space-between; gap: 10px; }
  .big { font: 600 24px var(--font-display); font-variant-numeric: tabular-nums; letter-spacing: -0.01em; }
  .lsub { font-size: 12px; font-variant-numeric: tabular-nums; }
  .gwrap { position: relative; }
  .stats { display: flex; gap: 14px; font-size: 11.5px; color: var(--text-3); font-variant-numeric: tabular-nums; }
  .stats b { color: var(--text-2); font-weight: 600; }
  .stats span:last-child { margin-left: auto; }
  .maplbl { font-size: 11px; }
  .col { display: flex; flex-direction: column; gap: 12px; min-width: 0; }
  .safe { max-width: 620px; align-self: flex-start; }
  div.safe { align-self: stretch; }
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
