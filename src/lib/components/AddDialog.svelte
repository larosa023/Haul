<script lang="ts">
  import Modal from './Modal.svelte';
  import Icon from './Icon.svelte';
  import Switch from './Switch.svelte';
  import { api, isTauri, type AddRequest, type ProbeInfo } from '../api';
  import { app, ui, toast, errorText, selectOnly } from '../state.svelte';
  import { bytes, linkKind } from '../format.js';

  let { url = '', request }: { url?: string; request?: AddRequest } = $props();

  // The dialog starts from what it was opened with.
  // svelte-ignore state_referenced_locally
  let text = $state(url ?? request?.url ?? '');
  // svelte-ignore state_referenced_locally
  let name = $state(request?.filename ?? '');
  let folder = $state('');
  let category = $state('Other');
  let sha256 = $state('');
  let paused = $state(false);
  let probe: ProbeInfo | null = $state(null);
  let probing = $state(false);
  let probeError = $state('');
  let busy = $state(false);
  let folderEdited = false;
  // svelte-ignore state_referenced_locally
  let nameEdited = !!request?.filename;

  const links = $derived(text.split(/\r?\n/).map((l) => l.trim()).filter(Boolean));
  const single = $derived(links.length === 1 ? links[0] : '');
  const kind = $derived(single ? linkKind(single) : null);
  const categories = $derived(Object.keys(app.settings?.folders ?? {}));
  // Real-Debrid for torrents: shown when connected and not switched off.
  const debridOffered = $derived(!!app.settings?.debridEnabled && !!app.settings?.debridKey && app.settings?.debridTorrents !== 'off');
  const anyTorrent = $derived(links.some((l) => linkKind(l) === 'torrent'));
  // svelte-ignore state_referenced_locally
  let viaDebrid = $state(app.settings?.debridTorrents === 'always' || (app.settings?.debridTorrents === 'ask' && localStorage.getItem('haul:via-debrid') !== '0'));
  $effect(() => {
    if (app.settings?.debridTorrents === 'ask') localStorage.setItem('haul:via-debrid', viaDebrid ? '1' : '0');
  });
  const useDebrid = $derived(debridOffered && viaDebrid);

  let timer: ReturnType<typeof setTimeout> | undefined;
  let seq = 0;
  // Look the link up (name, size, resumable, where it'd go) as soon as it's pasted.
  $effect(() => {
    const u = single;
    const k = kind;
    clearTimeout(timer);
    probe = null;
    probeError = '';
    if (k !== 'http') return;
    const my = ++seq;
    timer = setTimeout(async () => {
      probing = true;
      try {
        const p = await api.inspect(u, request?.referrer, request?.cookies);
        if (my !== seq) return;
        probe = p;
        if (!nameEdited) name = p.name;
        if (!folderEdited) folder = p.folder;
        category = p.category;
      } catch (e) {
        if (my === seq) probeError = errorText(e);
      } finally {
        if (my === seq) probing = false;
      }
    }, 350);
  });

  function onCategory(c: string) {
    category = c;
    if (!folderEdited && app.settings) folder = app.settings.sortEnabled ? (app.settings.folders[c] ?? app.settings.downloadDir) : app.settings.downloadDir;
  }

  async function browse() {
    if (!isTauri) return;
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({ directory: true, defaultPath: folder || app.settings?.downloadDir });
    if (typeof picked === 'string') {
      folder = picked;
      folderEdited = true;
    }
  }

  async function openTorrentFile() {
    if (!isTauri) return;
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({ multiple: true, filters: [{ name: 'Torrent files', extensions: ['torrent'] }] });
    const list = Array.isArray(picked) ? picked : picked ? [picked] : [];
    if (!list.length) return;
    close();
    for (const p of list) await api.addTorrent({ source: p }).catch((e) => toast(errorText(e), 'error'));
  }

  function close() {
    ui.dialog = null;
  }

  async function add() {
    if (!links.length || busy) return;
    busy = true;
    try {
      if (links.length === 1) {
        if (kind === 'torrent') {
          await api.addTorrent({ source: single, debrid: useDebrid });
        } else if (kind === 'http') {
          const id = await api.addHttp({
            url: single,
            name: nameEdited || request?.filename ? name : '',
            folder: folderEdited ? folder : '',
            category: category,
            sha256,
            paused,
            referrer: request?.referrer ?? '',
            cookies: request?.cookies ?? '',
            userAgent: request?.userAgent ?? '',
          });
          selectOnly(id);
        } else {
          toast('That doesn’t look like a link or magnet', 'error');
          busy = false;
          return;
        }
      } else {
        // Several links: each one goes where the sorting rules say.
        let n = 0;
        for (const l of links) {
          const k = linkKind(l);
          try {
            if (k === 'torrent') await api.addTorrent({ source: l, direct: true, debrid: useDebrid });
            else if (k === 'http') await api.addHttp({ url: l, paused });
            else continue;
            n++;
          } catch (e) {
            toast(`${l}: ${errorText(e)}`, 'error', 8000);
          }
        }
        toast(`Added ${n} download${n === 1 ? '' : 's'}`, 'success');
      }
      close();
    } catch (e) {
      toast(errorText(e), 'error', 8000);
    } finally {
      busy = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter' && (e.ctrlKey || !(e.target as HTMLElement).matches('textarea'))) {
      e.preventDefault();
      void add();
    }
  }
</script>

<Modal title="New download" width={600} onclose={close}>
  <div class="form" role="presentation" onkeydown={onKey}>
    <label class="field">
      <span>Link or magnet</span>
      <!-- svelte-ignore a11y_autofocus -->
      <textarea class="textarea mono" rows={links.length > 1 ? 5 : 2} bind:value={text} placeholder="https://… or magnet:?… (one per line for several)" spellcheck="false" autofocus></textarea>
    </label>

    {#if links.length > 1}
      <p class="muted note">{links.length} links. Each goes to its category folder.</p>
    {:else if kind === 'torrent'}
      <p class="muted note"><Icon name={useDebrid ? 'cloud' : 'magnet'} size={14} /> {useDebrid ? 'Real-Debrid downloads this torrent first, then Haul fetches the files at full speed.' : 'Torrent. You’ll pick files once Haul has its file list.'}</p>
    {:else if kind === 'http'}
      <div class="probe">
        {#if probing}<span class="spinner"></span><span class="muted">Checking the link…</span>
        {:else if probeError}<Icon name="alert" size={15} /><span class="bad">{probeError}</span>
        {:else if probe}
          <Icon name="file" size={15} />
          <span>{probe.size ? bytes(probe.size) : 'Size unknown'}</span>
          {#if probe.debrid}<span class="chip rd" title="This hoster link goes through Real-Debrid"><Icon name="cloud" size={12} />&nbsp;Real-Debrid</span>{/if}
          <span class="chip" title={probe.resumable ? 'Can be paused and resumed, and uses several connections' : 'This server sends the file in one piece, so it can’t resume'}>{probe.resumable ? 'Resumable' : 'No resume'}</span>
        {/if}
      </div>
      <label class="field">
        <span>Save as</span>
        <input class="input" bind:value={name} oninput={() => (nameEdited = true)} spellcheck="false" placeholder="File name" />
      </label>
      <div class="row2">
        <label class="field">
          <span>Category</span>
          <select class="select" value={category} onchange={(e) => onCategory((e.currentTarget as HTMLSelectElement).value)}>
            {#each categories as c (c)}<option value={c}>{c}</option>{/each}
          </select>
        </label>
        <label class="field grow">
          <span>Folder</span>
          <span class="folder">
            <input class="input" bind:value={folder} oninput={() => (folderEdited = true)} spellcheck="false" />
            <button class="btn" onclick={browse}>Browse</button>
          </span>
        </label>
      </div>
      <label class="field">
        <span>SHA-256 <span class="faint">(optional, checked when done)</span></span>
        <input class="input mono" bind:value={sha256} placeholder="64 hex characters" spellcheck="false" />
      </label>
    {/if}

    {#if anyTorrent && debridOffered}
      <label class="check rdswitch"><Switch bind:checked={viaDebrid} label="Download through Real-Debrid" /> Download {links.length > 1 ? 'torrents ' : ''}through Real-Debrid</label>
    {/if}
    {#if kind !== 'torrent'}
      <label class="check"><input type="checkbox" bind:checked={paused} /> Add paused</label>
    {/if}
  </div>

  {#snippet footer()}
    <button class="btn ghost left" onclick={openTorrentFile}><Icon name="folder" size={15} />Open .torrent…</button>
    <span class="grow"></span>
    <button class="btn" onclick={close}>Cancel</button>
    <button class="btn primary" disabled={!links.length || busy} onclick={add}>
      {#if busy}<span class="spinner"></span>{/if}Download
    </button>
  {/snippet}
</Modal>

<style>
  .form { display: flex; flex-direction: column; gap: 12px; }
  .field { display: flex; flex-direction: column; gap: 5px; font-size: 12.5px; }
  .field > span:first-child { font-weight: 600; color: var(--text-2); }
  .row2 { display: grid; grid-template-columns: 150px 1fr; gap: 12px; }
  .folder { display: flex; gap: 6px; }
  .note { margin: 0; font-size: 12.5px; display: flex; align-items: center; gap: 6px; }
  .probe { display: flex; align-items: center; gap: 8px; font-size: 12.5px; min-height: 20px; color: var(--text-2); }
  .bad { color: var(--danger); }
  .check { display: flex; align-items: center; gap: 8px; font-size: 13px; cursor: pointer; }
  .grow { flex: 1; }
  .rd { background: color-mix(in srgb, var(--accent) 20%, transparent); color: var(--accent); }
  .rdswitch { gap: 10px; }
  .left { margin-right: auto; }
</style>
