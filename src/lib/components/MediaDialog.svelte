<script lang="ts">
  import { onMount } from 'svelte';
  import Modal from './Modal.svelte';
  import Icon from './Icon.svelte';
  import Switch from './Switch.svelte';
  import Duplicate from './Duplicate.svelte';
  import { api, isTauri, type MediaInfo, type MediaOptions } from '../api';
  import { app, ui, toast, errorText, selectOnly, saveSettings, findDuplicate } from '../state.svelte';
  import { bytes, clock } from '../format.js';

  let { url, referrer = '' }: { url: string; referrer?: string } = $props();

  const FALLBACK: MediaOptions = { mode: 'video', height: 0, hdr: false, audioFormat: 'mp3', subtitles: false, subLangs: 'en', chapters: true, thumbnail: true, sponsorblock: 'off' };
  // svelte-ignore state_referenced_locally
  let opts: MediaOptions = $state({ ...FALLBACK, ...(app.settings?.mediaDefaults ?? {}) });
  let info = $state<MediaInfo | null>(null);
  let error = $state('');
  let loading = $state(true);
  let busy = $state(false);
  let folder = $state('');
  let paused = $state(false);
  let picked = $state<boolean[]>([]);
  let filter = $state('');

  const QUALITIES = [4320, 2160, 1440, 1080, 720, 480, 360];
  const qualityName = (h: number) => (h >= 4320 ? '8K' : h >= 2160 ? '4K' : h >= 1440 ? '1440p' : `${h}p`);
  // Heights the video really has (a playlist doesn't say, so offer the usual ones).
  const heights = $derived(info?.kind === 'video' && info.heights.length ? info.heights : QUALITIES);
  const tools = $derived(app.tools);
  const playlist = $derived(info?.kind === 'playlist');
  const count = $derived(picked.filter(Boolean).length);
  const shown = $derived(
    (info?.entries ?? []).map((e, i) => ({ ...e, i })).filter((e) => !filter || e.title.toLowerCase().includes(filter.toLowerCase())),
  );
  const estimate = $derived.by(() => {
    if (!info || playlist) return 0;
    if (opts.mode === 'audio') return info.audioSize;
    const h = opts.height || info.heights[0];
    return info.sizes[String(h)] ?? 0;
  });

  onMount(() => {
    let alive = true;
    (async () => {
      try {
        const r = await api.mediaInspect(url);
        if (!alive) return;
        info = r;
        picked = r.entries.map(() => true);
      } catch (e) {
        if (alive) error = errorText(e);
      } finally {
        if (alive) loading = false;
      }
    })();
    return () => (alive = false);
  });

  async function browse() {
    if (!isTauri) return;
    const { open } = await import('@tauri-apps/plugin-dialog');
    const p = await open({ directory: true, defaultPath: folder || app.settings?.downloadDir });
    if (typeof p === 'string') folder = p;
  }

  function setShown(v: boolean) {
    const ids = new Set(shown.map((e) => e.i));
    picked = picked.map((p, i) => (ids.has(i) ? v : p));
  }

  async function add() {
    if (!info || busy) return;
    busy = true;
    const options = $state.snapshot(opts) as MediaOptions;
    try {
      const list = playlist
        ? info.entries.filter((_, i) => picked[i]).map((e) => ({ url: e.url, title: e.title, options, folder, referrer, paused }))
        : [{ url: info.url || url, title: info.title, options, folder, referrer, paused }];
      if (!list.length) return;
      const ids = await api.addMedia(list);
      // Remember the choices for next time.
      void saveSettings({ mediaDefaults: options });
      if (ids.length === 1) selectOnly(ids[0]);
      else toast(`Queued ${ids.length} videos`, 'success');
      ui.dialog = null;
    } catch (e) {
      toast(errorText(e), 'error', 8000);
    } finally {
      busy = false;
    }
  }

  function retryAsFile() {
    ui.dialog = { kind: 'add', url };
  }
</script>

<Modal title={opts.mode === 'audio' ? 'Download audio' : 'Download video'} width={640} onclose={() => (ui.dialog = null)}>
  <div class="media">
    {#if tools?.busy}
      <div class="tools"><span class="spinner"></span>Getting {tools.busy} ready (first use only)… {Math.round(tools.progress * 100)}%
        <span class="bar"><span style="width: {tools.progress * 100}%"></span></span></div>
    {/if}

    {#if loading}
      <div class="head"><div class="thumb skeleton"></div><div class="meta"><div class="sk l"></div><div class="sk s"></div></div></div>
      <p class="muted center">Looking the video up…</p>
    {:else if error}
      <div class="err"><Icon name="alert" size={16} /><div><b>Couldn’t read that link.</b><br /><span class="muted">{error}</span></div></div>
    {:else if info}
      <div class="head">
        {#if info.thumbnail}<img class="thumb" src={info.thumbnail} alt="" referrerpolicy="no-referrer" />{:else}<div class="thumb ph"><Icon name={playlist ? 'list' : 'video'} size={26} /></div>{/if}
        <div class="meta">
          <div class="title" title={info.title}>{info.title}</div>
          <div class="muted sub">
            {#if info.site}<span>{info.site}</span>{/if}
            {#if info.uploader}<span>{info.uploader}</span>{/if}
            {#if playlist}<span>{info.entries.length} videos</span>{:else if info.duration}<span>{clock(info.duration)}</span>{/if}
            {#if info.isLive}<span class="live">Live</span>{/if}
          </div>
        </div>
      </div>

      {@const dup = playlist ? null : findDuplicate(info.url || url) ?? findDuplicate(url)}
      {#if dup}<Duplicate item={dup} />{/if}
      <div class="seg" role="radiogroup" aria-label="Video or audio">
        <button class:on={opts.mode === 'video'} role="radio" aria-checked={opts.mode === 'video'} onclick={() => (opts.mode = 'video')}><Icon name="video" size={15} />Video</button>
        <button class:on={opts.mode === 'audio'} role="radio" aria-checked={opts.mode === 'audio'} onclick={() => (opts.mode = 'audio')}><Icon name="music" size={15} />Audio only</button>
      </div>

      {#if opts.mode === 'video'}
        <div class="field">
          <span>Quality</span>
          <div class="chips">
            <button class="qc" class:on={opts.height === 0} onclick={() => (opts.height = 0)}>Best</button>
            {#each heights as h (h)}
              <button class="qc" class:on={opts.height === h} onclick={() => (opts.height = h)}>{qualityName(h)}
                {#if info.sizes[String(h)]}<small>{bytes(info.sizes[String(h)])}</small>{/if}</button>
            {/each}
          </div>
        </div>
        {#if info.hdr || playlist}
          <label class="row"><Switch bind:checked={opts.hdr} label="Prefer HDR" /><span>Prefer HDR <span class="faint">(when the video has it)</span></span></label>
        {/if}
      {:else}
        <div class="field">
          <span>Format</span>
          <div class="chips">
            {#each [['mp3', 'MP3'], ['m4a', 'M4A'], ['opus', 'Opus']] as [v, l] (v)}
              <button class="qc" class:on={opts.audioFormat === v} onclick={() => (opts.audioFormat = v as MediaOptions['audioFormat'])}>{l}</button>
            {/each}
          </div>
        </div>
      {/if}

      <div class="grid">
        {#if opts.mode === 'video'}
          <label class="row"><Switch bind:checked={opts.subtitles} label="Subtitles" /><span>Subtitles</span>
            {#if opts.subtitles}<input class="input mono langs" bind:value={opts.subLangs} placeholder="en,es or all" spellcheck="false" title="Languages (yt-dlp codes), or all" />{/if}</label>
        {/if}
        <label class="row"><Switch bind:checked={opts.chapters} label="Chapters" /><span>Chapters</span></label>
        <label class="row"><Switch bind:checked={opts.thumbnail} label="Thumbnail" /><span>Embed thumbnail</span></label>
        <div class="row sb">
          <span>SponsorBlock</span>
          <select class="select" bind:value={opts.sponsorblock}>
            <option value="off">Off</option>
            <option value="remove">Skip sponsors</option>
            <option value="mark">Mark as chapters</option>
          </select>
        </div>
      </div>

      {#if playlist}
        <div class="plist">
          <div class="ptools">
            <input class="input" bind:value={filter} placeholder="Filter videos" spellcheck="false" />
            <button class="btn ghost" onclick={() => setShown(true)}>All</button>
            <button class="btn ghost" onclick={() => setShown(false)}>None</button>
          </div>
          <div class="entries">
            {#each shown as e (e.i)}
              <label class="entry"><input type="checkbox" bind:checked={picked[e.i]} /><span class="n">{e.i + 1}</span><span class="t" title={e.title}>{e.title}</span><span class="d">{clock(e.duration)}</span></label>
            {/each}
          </div>
        </div>
      {/if}

      <label class="field">
        <span>Folder <span class="faint">(empty = your {opts.mode === 'audio' ? 'Music' : 'Video'} folder)</span></span>
        <span class="folder"><input class="input" bind:value={folder} spellcheck="false" placeholder={app.settings?.folders?.[opts.mode === 'audio' ? 'Music' : 'Video'] ?? ''} /><button class="btn" onclick={browse}>Browse</button></span>
      </label>
      <label class="check"><input type="checkbox" bind:checked={paused} /> Add paused</label>
    {/if}
  </div>

  {#snippet footer()}
    {#if error}<button class="btn ghost left" onclick={retryAsFile}><Icon name="file" size={15} />Download as a file</button>{/if}
    {#if estimate}<span class="muted est">About {bytes(estimate)}</span>{/if}
    <span class="grow"></span>
    <button class="btn" onclick={() => (ui.dialog = null)}>Cancel</button>
    <button class="btn primary" disabled={!info || busy || (playlist && !count)} onclick={add}>
      {#if busy}<span class="spinner"></span>{/if}{playlist ? `Download ${count}` : 'Download'}
    </button>
  {/snippet}
</Modal>

<style>
  .media { display: flex; flex-direction: column; gap: 14px; }
  .head { display: flex; gap: 14px; align-items: center; }
  .thumb { width: 160px; height: 90px; border-radius: 8px; object-fit: cover; flex-shrink: 0; background: var(--surface); }
  .ph { display: grid; place-items: center; color: var(--text-3); }
  .meta { min-width: 0; display: flex; flex-direction: column; gap: 6px; flex: 1; }
  .title { font: 600 15px var(--font-display); display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .sub { display: flex; flex-wrap: wrap; gap: 4px 12px; font-size: 12.5px; }
  .live { color: var(--danger); font-weight: 600; }
  .skeleton, .sk { background: linear-gradient(90deg, var(--surface) 0%, var(--surface-hover) 50%, var(--surface) 100%); background-size: 200% 100%; animation: shimmer 1.2s linear infinite; }
  .sk { height: 14px; border-radius: 4px; } .sk.l { width: 80%; } .sk.s { width: 40%; }
  @keyframes shimmer { to { background-position: -200% 0; } }
  .center { text-align: center; margin: 0; }
  .err { display: flex; gap: 10px; color: var(--danger); align-items: flex-start; font-size: 13px; }
  .err .muted { color: var(--text-2); user-select: text; }
  .tools { display: flex; align-items: center; gap: 8px; font-size: 12.5px; color: var(--text-2); }
  .bar { flex: 1; height: 4px; border-radius: 2px; background: var(--surface-strong); overflow: hidden; }
  .bar span { display: block; height: 100%; background: var(--accent); transition: width 300ms; }
  .seg { display: inline-flex; align-self: flex-start; padding: 3px; gap: 3px; border-radius: 8px; background: var(--surface); border: 1px solid var(--line); }
  .seg button { display: flex; align-items: center; gap: 6px; border: 0; background: transparent; padding: 6px 14px; border-radius: 6px; cursor: pointer; color: var(--text-2); }
  .seg button.on { background: var(--accent); color: var(--on-accent); font-weight: 600; }
  .field { display: flex; flex-direction: column; gap: 6px; font-size: 12.5px; }
  .field > span:first-child { font-weight: 600; color: var(--text-2); }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; }
  .qc { display: flex; flex-direction: column; align-items: center; min-width: 58px; padding: 6px 10px; border-radius: 7px; cursor: pointer; border: 1px solid var(--line); background: var(--surface); line-height: 1.2; transition: background var(--dur), border-color var(--dur); }
  .qc:hover { background: var(--surface-hover); }
  .qc.on { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 18%, var(--surface)); color: var(--text); font-weight: 600; }
  .qc small { font-size: 10.5px; color: var(--text-3); font-weight: 400; }
  .grid { display: grid; grid-template-columns: 1fr 1fr; gap: 10px 18px; }
  .row { display: flex; align-items: center; gap: 10px; font-size: 13px; cursor: pointer; min-height: 30px; }
  .row.sb { cursor: default; justify-content: space-between; }
  .row.sb .select { width: 160px; }
  .langs { width: 110px; height: 28px; margin-left: auto; }
  .plist { display: flex; flex-direction: column; gap: 6px; }
  .ptools { display: flex; gap: 6px; }
  .ptools .input { flex: 1; }
  .entries { max-height: 220px; overflow: auto; border: 1px solid var(--line); border-radius: 8px; contain: strict; height: 220px; }
  .entry { display: grid; grid-template-columns: 20px 34px 1fr auto; align-items: center; gap: 6px; padding: 0 10px; height: 30px; font-size: 12.5px; cursor: pointer; border-bottom: 1px solid var(--line); content-visibility: auto; contain-intrinsic-size: auto 30px; }
  .entry:hover { background: var(--surface-hover); }
  .entry .n { color: var(--text-3); font-variant-numeric: tabular-nums; text-align: right; }
  .entry .t { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .entry .d { color: var(--text-3); font-variant-numeric: tabular-nums; }
  .folder { display: flex; gap: 6px; }
  .folder .input { flex: 1; }
  .check { display: flex; align-items: center; gap: 8px; font-size: 13px; cursor: pointer; }
  .grow { flex: 1; }
  .left { margin-right: auto; }
  .est { font-size: 12.5px; }
</style>
