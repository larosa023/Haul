<script lang="ts">
  import { onMount } from 'svelte';
  import SpeedGraph from '../lib/components/SpeedGraph.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import SettingRow from '../lib/components/SettingRow.svelte';
  import Switch from '../lib/components/Switch.svelte';
  import { api, isTauri, type Adapter, type DebridService, type DebridState, type MediaOptions, type ProwlarrStatus, type Rule, type Settings } from '../lib/api';
  import { app, ui, saveSettings, toast, errorText } from '../lib/state.svelte';
  import { THEMES, ACCENT_SWATCHES, cssVars, effectiveTheme, resolveAccent, themeToJson, type ThemeDef } from '../lib/themecore.js';
  import { themes, allThemes, reloadThemes, findTheme } from '../lib/theme.svelte';

  const SECTIONS = [
    { id: 'general', label: 'General', icon: 'settings', blurb: 'Startup, the tray and new downloads' },
    { id: 'downloads', label: 'Downloads', icon: 'download', blurb: 'Folders, connections and the queue' },
    { id: 'media', label: 'Video & music', icon: 'video', blurb: 'YouTube and 1,800+ other sites, with yt-dlp' },
    { id: 'sorting', label: 'Sorting', icon: 'sort', blurb: 'Where finished files go, and extracting archives' },
    { id: 'speed', label: 'Speed & schedule', icon: 'gauge', blurb: 'Limits, and slower speeds at set times' },
    { id: 'torrents', label: 'Torrents', icon: 'magnet', blurb: 'Seeding, peers, privacy and ports' },
    { id: 'network', label: 'Network adapter', icon: 'network', blurb: 'Keep torrents on your VPN' },
    { id: 'safety', label: 'Safety', icon: 'shield', blurb: 'Safety scores, Windows Defender and VirusTotal' },
    { id: 'debrid', label: 'Debrid', icon: 'cloud', blurb: 'Real-Debrid, AllDebrid, Premiumize or TorBox' },
    { id: 'search', label: 'Torrent search', icon: 'search', blurb: 'Where Haul searches for torrents' },
    { id: 'browser', label: 'Browser', icon: 'puzzle', blurb: 'Send downloads from Chrome, Edge, Drift or Firefox' },
    { id: 'appearance', label: 'Appearance', icon: 'brush', blurb: 'Themes, accent color and layout' },
    { id: 'about', label: 'About & updates', icon: 'refresh', blurb: 'Version, updates and shortcuts' },
  ];
  const DAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

  const s = $derived(app.settings!);
  const section = $derived(SECTIONS.find((x) => x.id === ui.settingsTab) ?? SECTIONS[0]);

  let adapters: Adapter[] = $state([]);
  let handlers = $state({ magnet: false, torrent: false });
  let checking = $state(false);
  let rdKey = $state('');
  let rdBusy = $state(false);
  let rd: DebridState | null = $state(null);

  $effect(() => {
    if (ui.settingsTab === 'debrid' && isTauri && !rd) void api.debridStatus().then((s) => (rd = s));
  });

  const SERVICES: { id: DebridService; name: string; keyUrl: string; keyPage: string; keyName: string }[] = [
    { id: 'realdebrid', name: 'Real-Debrid', keyUrl: 'https://real-debrid.com/apitoken', keyPage: 'real-debrid.com/apitoken', keyName: 'API private token' },
    { id: 'alldebrid', name: 'AllDebrid', keyUrl: 'https://alldebrid.com/apikeys/', keyPage: 'alldebrid.com/apikeys', keyName: 'API key' },
    { id: 'premiumize', name: 'Premiumize', keyUrl: 'https://www.premiumize.me/account', keyPage: 'premiumize.me/account', keyName: 'API key' },
    { id: 'torbox', name: 'TorBox', keyUrl: 'https://torbox.app/settings', keyPage: 'torbox.app/settings', keyName: 'API key' },
  ];
  // svelte-ignore state_referenced_locally
  let svcTab: DebridService = $state(app.settings?.debridService ?? 'realdebrid');
  const svc = $derived(SERVICES.find((x) => x.id === svcTab)!);
  const active = $derived(SERVICES.find((x) => x.id === s.debridService)!);

  async function rdUse(id: DebridService) {
    rd = await api.debridUse(id);
    toast(`Haul now uses ${SERVICES.find((x) => x.id === id)?.name}`, 'success');
  }

  async function rdConnect() {
    rdBusy = true;
    try {
      rd = await api.debridConnect(rdKey, svcTab);
      rdKey = '';
      toast(`Connected to ${svc.name}${rd.account?.username ? ` as ${rd.account.username}` : ''}`, 'success');
    } catch (e) {
      toast(errorText(e), 'error', 8000);
    } finally {
      rdBusy = false;
    }
  }

  async function rdDisconnect() {
    app.settings = await api.debridDisconnect(svcTab);
    rd = await api.debridStatus();
  }

  // ---- video & music tools, Hugging Face
  let toolsBusy = $state(false);
  $effect(() => {
    if (ui.settingsTab === 'media' && isTauri && !app.tools) void api.toolsState().then((t) => (app.tools = t));
  });
  async function toolsUpdate() {
    toolsBusy = true;
    try {
      app.tools = await api.toolsUpdate();
      toast(`yt-dlp ${app.tools.ytdlp} and ffmpeg are ready`, 'success');
    } catch (e) {
      toast(errorText(e), 'error', 8000);
    } finally {
      toolsBusy = false;
    }
  }
  async function toolsRemove() {
    app.tools = await api.toolsRemove();
    toast('yt-dlp and ffmpeg removed. They download again when you next save a video.', 'info');
  }
  function setMedia(patch: Partial<MediaOptions>) {
    set('mediaDefaults', { ...s.mediaDefaults, ...patch });
  }

  let hfUser = $state('');
  let hfToken = $state('');
  let hfBusy = $state(false);
  $effect(() => {
    if (ui.settingsTab === 'downloads' && isTauri && s.hfToken && !hfUser) void api.hfStatus().then((n) => (hfUser = n)).catch(() => {});
  });
  async function hfConnect() {
    hfBusy = true;
    try {
      hfUser = await api.hfConnect(hfToken);
      hfToken = '';
      toast(`Connected to Hugging Face as ${hfUser}`, 'success');
    } catch (e) {
      toast(errorText(e), 'error', 8000);
    } finally {
      hfBusy = false;
    }
  }
  async function hfDisconnect() {
    app.settings = await api.hfDisconnect();
    hfUser = '';
  }

  // A made-up speed curve for the style previews.
  const SAMPLE = Array.from({ length: 48 }, (_, i) => 40 + 30 * Math.sin(i / 4) + 18 * Math.sin(i / 1.7) + (i > 30 ? 25 : 0));

  // ---- torrent search sources
  let flare = $state<{ running: boolean; path: string } | null>(null);
  let prow = $state<ProwlarrStatus | null>(null);
  let prowBusy = $state(false);
  $effect(() => {
    if (ui.settingsTab === 'search' && isTauri) {
      void api.flaresolverrStatus().then((f) => (flare = f)).catch(() => {});
      void api.prowlarrStatus().then((p) => (prow = p)).catch(() => {});
    }
  });
  async function startProwlarr() {
    prowBusy = true;
    try {
      prow = await api.prowlarrStart();
      if (!prow.running) toast('Prowlarr didn’t start. Open it once from the Start menu, then try again.', 'error');
    } finally {
      prowBusy = false;
    }
  }
  const prowText = $derived(
    !prow ? 'Checking…'
    : !prow.found ? 'Not installed. Install Prowlarr to search the sites you add to it; Haul handles the rest.'
    : prow.running ? `Connected · ${prow.sites} site${prow.sites === 1 ? '' : 's'} · Haul starts it hidden when you search`
    : 'Installed · not running right now. Haul starts it hidden when you search.'
  );
  async function pickFlare() {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const p = await open({ filters: [{ name: 'flaresolverr.exe', extensions: ['exe'] }] });
    if (typeof p === 'string') {
      set('flaresolverrPath', p);
      flare = await api.flaresolverrStatus();
    }
  }
  let ixName = $state('');
  let ixUrl = $state('');
  let ixKey = $state('');
  let ixBusy = $state(false);
  async function addIndexer() {
    ixBusy = true;
    try {
      app.settings = await api.searchAddIndexer({ name: ixName, url: ixUrl, key: ixKey });
      toast('Indexer added', 'success');
      ixName = ixUrl = ixKey = '';
    } catch (e) {
      toast(errorText(e), 'error', 8000);
    } finally {
      ixBusy = false;
    }
  }

  // ---- safety
  let vtKey = $state('');
  let vtBusy = $state(false);
  async function saveVt(key: string) {
    vtBusy = true;
    try {
      app.settings = await api.setVtKey(key);
      vtKey = '';
      toast(key ? 'VirusTotal connected' : 'VirusTotal key removed', 'success');
    } catch (e) {
      toast(errorText(e), 'error', 8000);
    } finally {
      vtBusy = false;
    }
  }

  // ---- mini window hotkey
  let recording = $state(false);
  function recordKey(e: KeyboardEvent) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') {
      recording = false;
      return;
    }
    if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return;
    const mods = [e.ctrlKey && 'Ctrl', e.altKey && 'Alt', e.shiftKey && 'Shift', e.metaKey && 'Super'].filter(Boolean);
    if (!mods.length) return; // a global hotkey needs a modifier
    const key = e.code.startsWith('Key') ? e.code.slice(3) : e.code.startsWith('Digit') ? e.code.slice(5) : e.key.length === 1 ? e.key.toUpperCase() : e.code;
    set('miniHotkey', [...mods, key].join('+'));
    recording = false;
  }

  const daysLeft = (secs: number) => Math.max(0, Math.floor(secs / 86400));

  onMount(() => {
    if (!isTauri) return;
    void api.adapters().then((a) => (adapters = a)).catch(() => {});
    void api.handlerState().then((h) => (handlers = h)).catch(() => {});
  });

  function set<K extends keyof Settings>(k: K, v: Settings[K]) {
    void saveSettings({ [k]: v } as Partial<Settings>);
  }

  function num(e: Event, min = 0, max = 1e9) {
    const v = Number((e.currentTarget as HTMLInputElement).value);
    return Math.max(min, Math.min(max, isFinite(v) ? Math.round(v) : min));
  }

  async function pickFolder(current: string): Promise<string | null> {
    if (!isTauri) return null;
    const { open } = await import('@tauri-apps/plugin-dialog');
    const p = await open({ directory: true, defaultPath: current });
    return typeof p === 'string' ? p : null;
  }

  async function pickFile(): Promise<string | null> {
    if (!isTauri) return null;
    const { open } = await import('@tauri-apps/plugin-dialog');
    const p = await open({ filters: [{ name: 'Blocklists', extensions: ['dat', 'p2p', 'txt', 'gz'] }] });
    return typeof p === 'string' ? p : null;
  }

  // ---- rules
  function setRules(rules: Rule[]) {
    set('rules', rules);
  }
  function updateRule(i: number, patch: Partial<Rule>) {
    setRules(s.rules.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  }
  function moveRule(i: number, d: number) {
    const r = [...s.rules];
    const j = i + d;
    if (j < 0 || j >= r.length) return;
    [r[i], r[j]] = [r[j], r[i]];
    setRules(r);
  }
  async function resetRules() {
    app.settings = await api.resetRules();
    toast('Sorting rules reset', 'success');
  }

  // ---- themes
  function preview(t: ThemeDef) {
    const shown = effectiveTheme(t, themes.systemLight);
    const v = cssVars(shown, resolveAccent(shown, s.accent, themes.systemAccent), false);
    return `--p-bg:${v['--solid-bg']};--p-side:${shown.colors.sidebar};--p-surface:${shown.colors.surface};--p-text:${shown.colors.text};--p-sub:${shown.colors.faint};--p-accent:${v['--accent']}`;
  }
  async function saveThemeFile() {
    const t = effectiveTheme(findTheme(s.theme), themes.systemLight);
    const accent = s.accent === 'theme' ? undefined : resolveAccent(t, s.accent, themes.systemAccent);
    try {
      const file = await api.saveTheme(`${t.name}${t.custom ? '' : ' copy'}`, themeToJson(t, accent));
      await reloadThemes();
      toast(`Saved ${file.split(/[\\/]/).pop()}`, 'success');
    } catch (e) {
      toast(errorText(e), 'error');
    }
  }

  // ---- updates
  async function checkNow() {
    checking = true;
    try {
      const u = await api.checkUpdate();
      app.update = u;
      if (u.status === 'none') toast('Haul is up to date', 'success');
      else if (u.status === 'error') toast(u.error, 'error');
    } finally {
      checking = false;
    }
  }

  const engine = $derived(app.totals?.engine);
  const currentAdapter = $derived(adapters.find((a) => a.name === s.bindAdapter));
</script>

<div class="settings">
  <nav class="side">
    {#each SECTIONS as x (x.id)}
      <button class="cat" class:on={section.id === x.id} onclick={() => (ui.settingsTab = x.id)}>
        <Icon name={x.icon} size={17} />{x.label}
        {#if x.id === 'about' && (app.update?.status === 'ready' || app.update?.status === 'available')}<span class="badge"></span>{/if}
      </button>
    {/each}
  </nav>

  <div class="content">
    {#key section.id}
      <header>
        <h2>{section.label}</h2>
        <p class="muted">{section.blurb}</p>
      </header>

      {#if section.id === 'general'}
        <div class="group">
          <SettingRow title="Mini window" description="A small always-on-top window with your active downloads, total speed and Pause all. Also in the tray menu.">
            <div class="pathrow">
              <button class="btn sm" class:primary={recording} onclick={() => (recording = !recording)} onkeydown={recordKey}>
                {recording ? 'Press keys…' : s.miniHotkey || 'No hotkey'}
              </button>
              {#if s.miniHotkey}<button class="icon-btn" title="No hotkey" onclick={() => set('miniHotkey', '')}><Icon name="x" size={14} /></button>{/if}
              <button class="btn sm" onclick={() => api.toggleMini()}><Icon name="mini" size={14} />Show</button>
            </div>
          </SettingRow>
          <SettingRow title="Keep running in the tray" description="Closing the window keeps Haul downloading in the tray. Quit from the tray icon.">
            <Switch checked={s.closeToTray} label="Keep running in the tray" onchange={(v) => set('closeToTray', v)} />
          </SettingRow>
          <SettingRow title="Start with Windows" description="Opens in the tray when you sign in.">
            <Switch checked={s.launchAtStartup} label="Start with Windows" onchange={(v) => set('launchAtStartup', v)} />
          </SettingRow>
          <SettingRow title="Start in the tray" description="Don’t show the window when Haul starts.">
            <Switch checked={s.startMinimized} label="Start in the tray" onchange={(v) => set('startMinimized', v)} />
          </SettingRow>
        </div>
        <div class="group">
          <SettingRow title="Offer to download copied links" description="When you copy a download link or magnet, Haul offers to grab it.">
            <Switch checked={s.watchClipboard} label="Watch the clipboard" onchange={(v) => set('watchClipboard', v)} />
          </SettingRow>
          <SettingRow title="Ask before adding" description="Show the Add window (name, folder, which torrent files) for downloads from the browser, clipboard and magnet links.">
            <Switch checked={s.showAddDialog} label="Ask before adding" onchange={(v) => set('showAddDialog', v)} />
          </SettingRow>
          <SettingRow title="Notify when a download finishes" description="With Open and Show in folder buttons.">
            <Switch checked={s.notifyComplete} label="Notifications" onchange={(v) => set('notifyComplete', v)} />
          </SettingRow>
        </div>
        <div class="group">
          <SettingRow title="Open magnet links and .torrent files with Haul" description={handlers.magnet && handlers.torrent ? 'Haul opens both.' : handlers.magnet ? 'Haul opens magnet links. Pick Haul for .torrent in Default Apps too.' : 'Windows asks you to confirm this in Default Apps.'}>
            {#if handlers.magnet && handlers.torrent}<span class="ok"><Icon name="check" size={15} />Default</span>
            {:else}<button class="btn sm" onclick={() => api.makeDefault().catch((e) => toast(errorText(e), 'error'))}>Make Haul the default</button>{/if}
          </SettingRow>
        </div>

      {:else if section.id === 'downloads'}
        <div class="group">
          <SettingRow title="Default folder" description="Used when sorting is off, and for anything without a category folder." wide>
            <div class="pathrow">
              <input class="input" value={s.downloadDir} onchange={(e) => set('downloadDir', (e.currentTarget as HTMLInputElement).value)} spellcheck="false" />
              <button class="btn" onclick={async () => { const p = await pickFolder(s.downloadDir); if (p) set('downloadDir', p); }}>Browse</button>
              <button class="icon-btn" title="Open" onclick={() => api.openFolder(s.downloadDir)}><Icon name="open" size={15} /></button>
            </div>
          </SettingRow>
        </div>
        <div class="group">
          <SettingRow title="Connections per download" description="Files are split into parts that download at once (when the server allows it). Auto picks by size: 1 for small files, up to 24 for huge ones.">
            <div class="pathrow">
              <select class="select short-sel" value={s.connections === 0 ? 'auto' : 'set'} onchange={(e) => set('connections', (e.currentTarget as HTMLSelectElement).value === 'auto' ? 0 : 8)}>
                <option value="auto">Auto</option><option value="set">Fixed</option>
              </select>
              {#if s.connections > 0}<input class="input n" type="number" min="1" max="32" value={s.connections} onchange={(e) => set('connections', num(e, 1, 32))} />{/if}
            </div>
          </SettingRow>
          <SettingRow title="Retries" description="How many times a failed connection or download is retried, with a growing pause between tries.">
            <input class="input n" type="number" min="0" max="50" value={s.maxRetries} onchange={(e) => set('maxRetries', num(e, 0, 50))} />
          </SettingRow>
          <SettingRow title="Downloads at once" description="Others wait in the queue (top of the list first, High priority before Normal). 0 = no limit.">
            <input class="input n" type="number" min="0" max="100" value={s.maxActive} onchange={(e) => set('maxActive', num(e, 0, 100))} />
          </SettingRow>
          <SettingRow title="Hugging Face token" description={hfUser ? `Connected as ${hfUser}. Gated and private models download with your account.` : 'Only needed for gated or private models. Make a read token at huggingface.co/settings/tokens.'} wide>
            {#if s.hfToken}
              <div class="pathrow"><span class="grow muted small">{hfUser ? `Signed in as ${hfUser}` : 'Token saved'}</span><button class="btn sm ghost" onclick={hfDisconnect}>Disconnect</button></div>
            {:else}
              <div class="pathrow">
                <input class="input mono" type="password" bind:value={hfToken} placeholder="hf_…" spellcheck="false" autocomplete="off" onkeydown={(e) => e.key === 'Enter' && hfToken.trim() && hfConnect()} />
                <button class="btn" disabled={!hfToken.trim() || hfBusy} onclick={hfConnect}>{#if hfBusy}<span class="spinner"></span>{/if}Connect</button>
              </div>
            {/if}
          </SettingRow>
          <SettingRow title="User agent" description="What Haul tells servers it is. Leave empty for the default; downloads from the browser use the browser’s." wide>
            <input class="input mono" value={s.userAgent} placeholder="Default" onchange={(e) => set('userAgent', (e.currentTarget as HTMLInputElement).value)} spellcheck="false" />
          </SettingRow>
        </div>

      {:else if section.id === 'media'}
        {@const m = s.mediaDefaults}
        {@const t = app.tools}
        <div class="status-card">
          <span class="dot" class:good={!!t?.ytdlp && !!t?.ffmpeg}></span>
          <div>
            <strong>{t?.ytdlp ? `yt-dlp ${t.ytdlp}` : 'yt-dlp not downloaded yet'}{t?.ffmpeg ? ` · ffmpeg ${t.ffmpeg}` : ''}</strong>
            <div class="muted small">
              {#if t?.busy}Downloading {t.busy}… {Math.round(t.progress * 100)}%
              {:else if t?.error}<span class="bad">{t.error}</span>
              {:else if t?.ytdlp}Checked against their SHA-256 before use. Stored in Haul’s data folder.
              {:else}Haul downloads them (about 140 MB) the first time you save a video, and checks their SHA-256.{/if}
            </div>
          </div>
          <span class="grow"></span>
          <button class="btn sm" disabled={toolsBusy || !!t?.busy} onclick={toolsUpdate}>{#if toolsBusy}<span class="spinner"></span>{/if}{t?.ytdlp ? 'Update now' : 'Download now'}</button>
          {#if t?.ytdlp}<button class="btn sm ghost" onclick={toolsRemove}>Remove</button>{/if}
        </div>
        <div class="group" style="margin-top: 10px">
          <SettingRow title="Keep yt-dlp up to date" description="Sites change often and break older versions. Haul checks for a new yt-dlp every few hours, and right away when a site stops working.">
            <Switch checked={s.ytdlpAutoUpdate} label="Keep yt-dlp up to date" onchange={(v) => set('ytdlpAutoUpdate', v)} />
          </SettingRow>
        </div>
        <h3>Defaults</h3>
        <p class="faint small">The video window starts with these, and remembers what you pick there. Links sent without the window use them as they are.</p>
        <div class="group">
          <SettingRow title="Save as" description="Video (MP4, merged by ffmpeg) or just the audio.">
            <div class="seg">
              <button class:on={m.mode === 'video'} onclick={() => setMedia({ mode: 'video' })}>Video</button>
              <button class:on={m.mode === 'audio'} onclick={() => setMedia({ mode: 'audio' })}>Audio</button>
            </div>
          </SettingRow>
          <SettingRow title="Video quality" description="The highest quality to fetch. Haul takes the best the video has, up to this.">
            <select class="select" value={String(m.height)} onchange={(e) => setMedia({ height: Number((e.currentTarget as HTMLSelectElement).value) })}>
              {#each [[0, 'Best available'], [4320, '8K'], [2160, '4K'], [1440, '1440p'], [1080, '1080p'], [720, '720p'], [480, '480p'], [360, '360p']] as [v, l] (v)}<option value={String(v)}>{l}</option>{/each}
            </select>
          </SettingRow>
          <SettingRow title="Prefer HDR" description="Pick HDR versions when a video has them (they need an HDR display to look right).">
            <Switch checked={m.hdr} label="Prefer HDR" onchange={(v) => setMedia({ hdr: v })} />
          </SettingRow>
          <SettingRow title="Audio format" description="For audio-only downloads.">
            <div class="seg">
              {#each [['mp3', 'MP3'], ['m4a', 'M4A'], ['opus', 'Opus']] as [v, l] (v)}<button class:on={m.audioFormat === v} onclick={() => setMedia({ audioFormat: v as MediaOptions['audioFormat'] })}>{l}</button>{/each}
            </div>
          </SettingRow>
          <SettingRow title="Subtitles" description="Embed subtitles in the video. Languages as yt-dlp codes (en, en,es) or all.">
            <div class="pathrow">
              {#if m.subtitles}<input class="input mono n2" value={m.subLangs} onchange={(e) => setMedia({ subLangs: (e.currentTarget as HTMLInputElement).value || 'en' })} spellcheck="false" />{/if}
              <Switch checked={m.subtitles} label="Subtitles" onchange={(v) => setMedia({ subtitles: v })} />
            </div>
          </SettingRow>
          <SettingRow title="Chapters" description="Keep the video’s chapters so players can jump between them.">
            <Switch checked={m.chapters} label="Chapters" onchange={(v) => setMedia({ chapters: v })} />
          </SettingRow>
          <SettingRow title="Thumbnail" description="Embed the thumbnail as cover art.">
            <Switch checked={m.thumbnail} label="Thumbnail" onchange={(v) => setMedia({ thumbnail: v })} />
          </SettingRow>
          <SettingRow title="SponsorBlock" description="Uses SponsorBlock’s community data to cut out sponsor, self-promo and “subscribe” segments, or mark them as chapters.">
            <div class="seg">
              {#each [['off', 'Off'], ['remove', 'Skip'], ['mark', 'Mark']] as [v, l] (v)}<button class:on={m.sponsorblock === v} onclick={() => setMedia({ sponsorblock: v as MediaOptions['sponsorblock'] })}>{l}</button>{/each}
            </div>
          </SettingRow>
        </div>
        <p class="faint small">Videos go to your Video folder and audio to Music, following the sorting rules. HLS (.m3u8) and DASH streams download their pieces in parallel and are joined into one file.</p>

      {:else if section.id === 'sorting'}
        <div class="group">
          <SettingRow title="Sort downloads into folders" description="Each download goes to its category’s folder, picked by the rules below.">
            <Switch checked={s.sortEnabled} label="Sort into folders" onchange={(v) => set('sortEnabled', v)} />
          </SettingRow>
        </div>
        {#if s.sortEnabled}
          <h3>Folders</h3>
          <div class="group">
            {#each Object.keys(s.folders) as c (c)}
              <div class="frow">
                <span class="fcat">{c}</span>
                <input class="input" value={s.folders[c]} onchange={(e) => set('folders', { ...s.folders, [c]: (e.currentTarget as HTMLInputElement).value })} spellcheck="false" />
                <button class="btn sm" onclick={async () => { const p = await pickFolder(s.folders[c]); if (p) set('folders', { ...s.folders, [c]: p }); }}>Browse</button>
                <button class="icon-btn" title="Open" onclick={() => api.openFolder(s.folders[c])}><Icon name="open" size={15} /></button>
              </div>
            {/each}
          </div>
          <h3>Rules <span class="faint">(first match wins; no match = Other)</span></h3>
          <div class="group rules">
            <div class="rhead"><span></span><span>Category</span><span>Extensions</span><span>Min MB</span><span>Max MB</span><span>Site contains</span><span></span></div>
            {#each s.rules as r, i (i)}
              <div class="rrow" class:off={!r.enabled}>
                <input type="checkbox" checked={r.enabled} onchange={(e) => updateRule(i, { enabled: (e.currentTarget as HTMLInputElement).checked })} aria-label="Rule on" />
                <select class="select" value={r.category} onchange={(e) => updateRule(i, { category: (e.currentTarget as HTMLSelectElement).value })}>
                  {#each Object.keys(s.folders) as c (c)}<option value={c}>{c}</option>{/each}
                </select>
                <input class="input" value={r.extensions.join(' ')} placeholder="any" spellcheck="false"
                  onchange={(e) => updateRule(i, { extensions: (e.currentTarget as HTMLInputElement).value.toLowerCase().split(/[\s,;]+/).map((x) => x.replace(/^\./, '')).filter(Boolean) })} />
                <input class="input" type="number" min="0" value={r.minSizeMb || ''} placeholder="—" onchange={(e) => updateRule(i, { minSizeMb: Number((e.currentTarget as HTMLInputElement).value) || 0 })} />
                <input class="input" type="number" min="0" value={r.maxSizeMb || ''} placeholder="—" onchange={(e) => updateRule(i, { maxSizeMb: Number((e.currentTarget as HTMLInputElement).value) || 0 })} />
                <input class="input" value={r.site} placeholder="any site" spellcheck="false" onchange={(e) => updateRule(i, { site: (e.currentTarget as HTMLInputElement).value.trim() })} />
                <span class="ract">
                  <button class="icon-btn s" title="Up" disabled={i === 0} onclick={() => moveRule(i, -1)}><Icon name="up" size={14} /></button>
                  <button class="icon-btn s" title="Down" disabled={i === s.rules.length - 1} onclick={() => moveRule(i, 1)}><Icon name="down" size={14} /></button>
                  <button class="icon-btn s" title="Delete" onclick={() => setRules(s.rules.filter((_, j) => j !== i))}><Icon name="trash" size={14} /></button>
                </span>
              </div>
            {/each}
            <div class="rfoot">
              <button class="btn sm" onclick={() => setRules([{ enabled: true, category: 'Other', extensions: [], minSizeMb: 0, maxSizeMb: 0, site: '' }, ...s.rules])}><Icon name="plus" size={14} />Add rule</button>
              <button class="btn sm ghost" onclick={resetRules}>Reset to defaults</button>
            </div>
          </div>
        {/if}
        <h3>Archives</h3>
        <div class="group">
          <SettingRow title="Extract finished archives" description="zip, 7z and rar (multi-part too). Files go next to the archive, in a folder when there’s more than one.">
            <Switch checked={s.autoExtract} label="Auto-extract" onchange={(v) => set('autoExtract', v)} />
          </SettingRow>
          {#if s.autoExtract}
            <SettingRow title="Delete the archive after extracting" description="Only for direct downloads; torrents keep their archives so they can keep seeding.">
              <Switch checked={s.deleteArchiveAfterExtract} label="Delete archive" onchange={(v) => set('deleteArchiveAfterExtract', v)} />
            </SettingRow>
          {/if}
        </div>

      {:else if section.id === 'speed'}
        <div class="group">
          <SettingRow title="Download limit" description="For everything together. 0 = no limit.">
            <span class="unit"><input class="input n" type="number" min="0" value={s.downLimit} onchange={(e) => set('downLimit', num(e))} /> KB/s</span>
          </SettingRow>
          <SettingRow title="Upload limit" description="Torrent uploads. 0 = no limit.">
            <span class="unit"><input class="input n" type="number" min="0" value={s.upLimit} onchange={(e) => set('upLimit', num(e))} /> KB/s</span>
          </SettingRow>
        </div>
        <h3>Limited speed <span class="faint">(the turtle button, and the schedule)</span></h3>
        <div class="group">
          <SettingRow title="Download limit when limited">
            <span class="unit"><input class="input n" type="number" min="0" value={s.altDownLimit} onchange={(e) => set('altDownLimit', num(e))} /> KB/s</span>
          </SettingRow>
          <SettingRow title="Upload limit when limited">
            <span class="unit"><input class="input n" type="number" min="0" value={s.altUpLimit} onchange={(e) => set('altUpLimit', num(e))} /> KB/s</span>
          </SettingRow>
          <SettingRow title="Limit speed now" description="Same as the turtle button.">
            <Switch checked={s.altEnabled} label="Limit speed now" onchange={(v) => set('altEnabled', v)} />
          </SettingRow>
        </div>
        <div class="group">
          <SettingRow title="Use a schedule" description="Limited speed between these times (full speed the rest of the time). For example, 08:00 to 23:00 limits the day and runs at full speed overnight.">
            <Switch checked={s.schedule.enabled} label="Use a schedule" onchange={(v) => set('schedule', { ...s.schedule, enabled: v })} />
          </SettingRow>
          {#if s.schedule.enabled}
            <SettingRow title="From">
              <input class="input t" type="time" value={s.schedule.from} onchange={(e) => set('schedule', { ...s.schedule, from: (e.currentTarget as HTMLInputElement).value })} />
            </SettingRow>
            <SettingRow title="To">
              <input class="input t" type="time" value={s.schedule.to} onchange={(e) => set('schedule', { ...s.schedule, to: (e.currentTarget as HTMLInputElement).value })} />
            </SettingRow>
            <SettingRow title="On">
              <div class="days">
                {#each DAYS as d, i (d)}
                  <button class:on={s.schedule.days[i]} onclick={() => set('schedule', { ...s.schedule, days: s.schedule.days.map((v, j) => (j === i ? !v : v)) })}>{d}</button>
                {/each}
              </div>
            </SettingRow>
          {/if}
        </div>

      {:else if section.id === 'torrents'}
        <div class="status-card">
          <span class="dot" class:good={engine?.running}></span>
          <div>
            <strong>{engine?.running ? `Torrents are running on port ${engine.port}` : 'Torrents are stopped'}</strong>
            <div class="muted small">
              {#if engine?.error}{engine.error}{:else}
                DHT {engine?.dht ? 'on' : 'off'} · UPnP {engine?.upnp ? 'on' : 'off'}{engine?.natpmp ? ` · NAT-PMP: ${engine.natpmp}` : ''}{engine?.blocklist ? ` · Blocklist: ${engine.blocklist}` : ''}{engine?.adapter ? ` · Bound to ${engine.adapter}` : ''}
              {/if}
            </div>
          </div>
          <span class="grow"></span>
          <button class="btn sm" onclick={() => api.restartTorrents().then(() => toast('Torrents restarted', 'success')).catch((e) => toast(errorText(e), 'error'))}>Restart</button>
        </div>
        <h3>Seeding</h3>
        <div class="group">
          <SettingRow title="Stop seeding at ratio" description="Uploaded ÷ downloaded. 0 = keep seeding. Each torrent can override this.">
            <input class="input n" type="number" min="0" step="0.1" value={s.ratioLimit} onchange={(e) => set('ratioLimit', Math.max(0, Number((e.currentTarget as HTMLInputElement).value) || 0))} />
          </SettingRow>
          <SettingRow title="Stop seeding after" description="Minutes of seeding. 0 = no limit.">
            <span class="unit"><input class="input n" type="number" min="0" value={s.seedTimeLimit} onchange={(e) => set('seedTimeLimit', num(e))} /> min</span>
          </SettingRow>
          <SettingRow title="Torrents seeding at once" description="0 = no limit.">
            <input class="input n" type="number" min="0" value={s.maxActiveSeeds} onchange={(e) => set('maxActiveSeeds', num(e))} />
          </SettingRow>
        </div>
        <h3>Peers and privacy</h3>
        <div class="group">
          <SettingRow title="Encryption" description="Prefer: encrypt when the other side can. Require: only talk to peers that encrypt (fewer peers). Off: never encrypt.">
            <div class="seg">
              {#each [['prefer', 'Prefer'], ['require', 'Require'], ['disabled', 'Off']] as [v, l] (v)}
                <button class:on={s.encryption === v} onclick={() => set('encryption', v as Settings['encryption'])}>{l}</button>
              {/each}
            </div>
          </SettingRow>
          <SettingRow title="DHT" description="Find peers without trackers (needed for most magnet links). Peer exchange (PEX) is always on.">
            <Switch checked={s.dht} label="DHT" onchange={(v) => set('dht', v)} />
          </SettingRow>
          <SettingRow title="IP blocklist" description="Refuse peers on a blocklist (eMule .dat, PeerGuardian .p2p, plain or .gz). A link or a file.">
            <Switch checked={s.blocklistEnabled} label="IP blocklist" onchange={(v) => set('blocklistEnabled', v)} />
          </SettingRow>
          {#if s.blocklistEnabled}
            <SettingRow title="Blocklist" wide>
              <div class="pathrow">
                <input class="input mono" value={s.blocklist} placeholder="https://… or C:\path\list.p2p" spellcheck="false" onchange={(e) => set('blocklist', (e.currentTarget as HTMLInputElement).value.trim())} />
                <button class="btn" onclick={async () => { const p = await pickFile(); if (p) set('blocklist', p); }}>Browse</button>
              </div>
            </SettingRow>
          {/if}
        </div>
        <h3>Ports</h3>
        <div class="group">
          <SettingRow title="Listening port" description="Other peers connect to you on this port (TCP and uTP).">
            <span class="unit">
              <input class="input n" type="number" min="1024" max="65535" value={s.listenPort} onchange={(e) => set('listenPort', num(e, 1024, 65535))} />
              <button class="btn sm ghost" onclick={() => set('listenPort', 20000 + Math.floor(Math.random() * 40000))}>Random</button>
            </span>
          </SettingRow>
          <SettingRow title="UPnP port forwarding" description="Ask the router to open the port.">
            <Switch checked={s.upnp} label="UPnP" onchange={(v) => set('upnp', v)} />
          </SettingRow>
          <SettingRow title="NAT-PMP port forwarding" description="The same, for routers that use NAT-PMP.">
            <Switch checked={s.natpmp} label="NAT-PMP" onchange={(v) => set('natpmp', v)} />
          </SettingRow>
        </div>

      {:else if section.id === 'network'}
        <div class="group">
          <SettingRow title="Torrent traffic goes through" description="Pick your VPN’s adapter to keep torrents on it. If that adapter disconnects, torrents stop until it’s back. Direct downloads aren’t affected.">
            <select class="select wide" value={s.bindAdapter} onchange={(e) => set('bindAdapter', (e.currentTarget as HTMLSelectElement).value)}>
              <option value="">Any adapter</option>
              {#each adapters as a (a.index)}<option value={a.name}>{a.name}{a.up ? '' : ' (disconnected)'}</option>{/each}
              {#if s.bindAdapter && !adapters.some((a) => a.name === s.bindAdapter)}<option value={s.bindAdapter}>{s.bindAdapter} (not found)</option>{/if}
            </select>
          </SettingRow>
          {#if s.bindAdapter}
            <SettingRow title="Status" description={currentAdapter ? `${currentAdapter.description}${currentAdapter.ipv4 ? ` · ${currentAdapter.ipv4}` : ''}` : 'Not connected'}>
              {#if engine?.adapterDown}<span class="bad"><Icon name="shield" size={15} />Disconnected: torrents stopped</span>
              {:else if engine?.running}<span class="ok"><Icon name="shield" size={15} />Torrents on {engine.adapter}</span>{/if}
            </SettingRow>
          {/if}
        </div>
        <p class="muted small">Adapters: <button class="link" onclick={() => api.adapters().then((a) => (adapters = a))}>refresh list</button></p>

      {:else if section.id === 'safety'}
        <p class="muted small">Every link gets a safety score from <strong>A</strong> (looks safe) to <strong>F</strong> (likely dangerous) when you paste it, and again once the file has downloaded and been checked. Click a score to see the reasons.</p>
        <div class="group">
          <SettingRow title="Check finished downloads" description="Looks at the file itself: does it match its name, is a program signed and by whom, what's inside archives. Dangerous files aren't auto-extracted, and you get a warning.">
            <Switch checked={s.safetyScan} label="Check finished downloads" onchange={(v) => set('safetyScan', v)} />
          </SettingRow>
          <SettingRow title="Scan with Windows Defender" description="Part of the check. If Defender finds something it quarantines it, as usual.">
            <Switch checked={s.safetyDefender} label="Scan with Windows Defender" onchange={(v) => set('safetyDefender', v)} />
          </SettingRow>
          <SettingRow title="Mark downloads as from the internet" description="Like browsers do, so Windows SmartScreen checks programs and Office opens documents in Protected View.">
            <Switch checked={s.safetyMotw} label="Mark as from the internet" onchange={(v) => set('safetyMotw', v)} />
          </SettingRow>
          <SettingRow title="VirusTotal (optional)" description={s.vtKey ? 'Connected. Finished files are looked up by their SHA-256; the file itself is never uploaded.' : 'Add a free API key from virustotal.com to see what 70+ antivirus engines say. Only the file’s SHA-256 is sent, never the file.'} wide>
            {#if s.vtKey}
              <div class="pathrow"><span class="grow muted small">Key saved (encrypted for your Windows account)</span><button class="btn sm ghost" disabled={vtBusy} onclick={() => saveVt('')}>Remove</button></div>
            {:else}
              <div class="pathrow">
                <input class="input mono" type="password" bind:value={vtKey} placeholder="VirusTotal API key" spellcheck="false" autocomplete="off" onkeydown={(e) => e.key === 'Enter' && vtKey.trim() && saveVt(vtKey)} />
                <button class="btn" disabled={!vtKey.trim() || vtBusy} onclick={() => saveVt(vtKey)}>{#if vtBusy}<span class="spinner"></span>{/if}Connect</button>
                <button class="btn ghost" onclick={() => api.openLink('https://www.virustotal.com/gui/my-apikey')}>Get a key</button>
              </div>
            {/if}
          </SettingRow>
        </div>
        <p class="faint small">A score is a guide, not a guarantee: a well-made threat can still look clean, and a safe tool can look unusual. When in doubt, don't run programs from sites you don't know.</p>

      {:else if section.id === 'debrid'}
        <div class="seg svcs" role="tablist" aria-label="Debrid service">
          {#each SERVICES as x (x.id)}
            <button class:on={svcTab === x.id} role="tab" aria-selected={svcTab === x.id} onclick={() => (svcTab = x.id)}>
              {x.name}{#if s.debridKeys?.[x.id]}<span class="saved" class:inuse={s.debridService === x.id} title={s.debridService === x.id ? 'In use' : 'Key saved'}></span>{/if}
            </button>
          {/each}
        </div>
        {#if s.debridKeys?.[svcTab] && svcTab === s.debridService && rd?.connected !== false}
          <div class="status-card">
            <span class="dot" class:good={!!rd?.account && rd.account.kind === 'premium'}></span>
            <div>
              {#if rd?.account}
                <strong>{svc.name}{rd.account.username ? ` · ${rd.account.username}` : ''}</strong>
                <div class="muted small">
                  {rd.account.kind === 'premium' ? `Premium${rd.account.premium ? ` · ${daysLeft(rd.account.premium)} days left` : ''}${rd.account.expiration ? ` (until ${new Date(rd.account.expiration).toLocaleDateString()})` : ''}` : 'Free account: most features need premium'}
                </div>
              {:else if rd?.error}
                <strong>{svc.name}</strong><div class="bad small">{rd.error}</div>
              {:else}
                <strong>{svc.name}</strong><div class="muted small">Checking your account…</div>
              {/if}
            </div>
            <span class="grow"></span>
            <button class="btn sm ghost" onclick={rdDisconnect}>Disconnect</button>
          </div>
          <div class="group" style="margin-top: 10px">
            <SettingRow title="Use {svc.name}" description="Switch it off to download everything directly again, without removing your key.">
              <Switch checked={s.debridEnabled} label="Use {svc.name}" onchange={(v) => set('debridEnabled', v)} />
            </SettingRow>
            <SettingRow title="File hoster links" description="Links from hosters {svc.name} supports (1fichier, Rapidgator, Mega and many more) download at premium speed, with Haul’s multiple connections. Expired links are renewed automatically.">
              <Switch checked={s.debridHosters} label="File hoster links" onchange={(v) => set('debridHosters', v)} />
            </SettingRow>
            <SettingRow title="Torrents" description="{svc.name} downloads the torrent on its servers (cached ones are instant), then Haul fetches the files over HTTPS. No seeding, and no torrent traffic from your PC.">
              <div class="seg">
                {#each [['off', 'Off'], ['ask', 'Ask each time'], ['always', 'Always']] as [v, l] (v)}
                  <button class:on={s.debridTorrents === v} onclick={() => set('debridTorrents', v as Settings['debridTorrents'])}>{l}</button>
                {/each}
              </div>
            </SettingRow>
          </div>
          <p class="faint small">“Ask each time” adds a {svc.name} switch to the New download window, and magnet links you click open it.{svcTab !== 'realdebrid' ? ` ${svc.name} fetches the whole torrent; choosing files before it starts is a Real-Debrid feature.` : ''}</p>
        {:else if s.debridKeys?.[svcTab]}
          <div class="group connect">
            <p>Your <strong>{svc.name}</strong> key is saved. Haul is using <strong>{active.name}</strong> right now.</p>
            <div class="pathrow">
              <button class="btn primary" onclick={() => rdUse(svcTab)}>Use {svc.name} instead</button>
              <button class="btn ghost" onclick={rdDisconnect}>Remove key</button>
            </div>
          </div>
        {:else}
          <div class="group connect">
            <p>Connect your <strong>{svc.name}</strong> account to download from file hosters at premium speed, and to have torrents downloaded on {svc.name}’s servers.</p>
            <ol>
              <li>Sign in at {svc.keyPage.split('/')[0]} and open <button class="link" onclick={() => api.openLink(svc.keyUrl)}>{svc.keyPage}</button>.</li>
              <li>Copy your {svc.keyName} and paste it here.</li>
            </ol>
            <div class="pathrow">
              <input class="input mono" type="password" bind:value={rdKey} placeholder={svc.keyName} spellcheck="false" autocomplete="off"
                onkeydown={(e) => e.key === 'Enter' && rdKey.trim() && rdConnect()} />
              <button class="btn primary" disabled={!rdKey.trim() || rdBusy} onclick={rdConnect}>{#if rdBusy}<span class="spinner"></span>{/if}Connect</button>
            </div>
            {#if rd?.error && svcTab === s.debridService}<p class="bad small">{rd.error}</p>{/if}
            <p class="faint small">The key is stored encrypted for your Windows account, and is only ever sent to {svc.name}. Connecting makes {svc.name} the service Haul uses; other saved keys stay for switching back.</p>
          </div>
        {/if}

      {:else if section.id === 'search'}
        <p class="muted small">Search torrents from the <strong>Search</strong> button in the sidebar. Haul asks every source below at once and sorts the results by seeders.</p>
        <div class="group">
          <SettingRow title="Internet Archive" description="Public-domain films, music, software and books. Every item has a torrent.">
            <Switch checked={s.searchArchive} label="Internet Archive" onchange={(v) => set('searchArchive', v)} />
          </SettingRow>
          {#each s.searchIndexers as ix (ix.url)}
            <SettingRow title={ix.name} description={prow?.found && s.prowlarrAuto && /\/\/(localhost|127\.0\.0\.1)[:/].*\/\d+\/api/.test(ix.url) ? 'Not needed any more: “Search with Prowlarr” below covers it. You can remove it.' : ix.url}>
              <div class="pathrow">
                <button class="icon-btn" title="Remove" onclick={async () => (app.settings = await api.searchRemoveIndexer(ix.url))}><Icon name="trash" size={14} /></button>
                <Switch checked={ix.enabled} label={ix.name} onchange={async (v) => (app.settings = await api.searchToggleIndexer(ix.url, v))} />
              </div>
            </SettingRow>
          {/each}
        </div>
        <h3>Prowlarr</h3>
        <div class="group">
          <SettingRow title="Search with Prowlarr" description={prowText} wide>
            <div class="pathrow">
              {#if prow && !prow.found}
                <button class="btn sm" onclick={() => api.openLink('https://prowlarr.com/#downloads-v1-windows')}>Get Prowlarr</button>
              {:else if prow?.running}
                <button class="btn sm ghost" onclick={() => api.openLink(prow!.address)}>Add sites…</button>
              {:else if prow}
                <button class="btn sm" disabled={prowBusy} onclick={startProwlarr}>{#if prowBusy}<span class="spinner"></span>{/if}Start now</button>
              {/if}
              <Switch checked={s.prowlarrAuto} label="Search with Prowlarr" onchange={(v) => set('prowlarrAuto', v)} />
            </div>
          </SettingRow>
        </div>
        <p class="faint small">No links or keys to copy: Haul reads them from Prowlarr itself and searches every site you’ve added there. Sites Prowlarr has paused after errors are re-tested automatically.</p>
        <h3>FlareSolverr</h3>
        <div class="group">
          <SettingRow title="Start FlareSolverr when searching" description={flare?.path ? `${flare.running ? 'Running' : 'Not running right now'} · ${flare.path}` : 'Lets Prowlarr get past Cloudflare checks. Haul starts it hidden (no window) before a search if it isn’t already running.'} wide>
            <div class="pathrow">
              {#if !flare?.path}<button class="btn sm" onclick={pickFlare}>Find flaresolverr.exe…</button>{:else}<button class="btn sm ghost" onclick={pickFlare}>Change…</button>{/if}
              <Switch checked={s.flaresolverrAuto} label="Start FlareSolverr when searching" onchange={(v) => set('flaresolverrAuto', v)} />
            </div>
          </SettingRow>
        </div>
        <h3>Other sources (optional)</h3>
        <div class="group connect">
          <p class="small">Prowlarr above needs nothing here. To add <button class="link" onclick={() => api.openLink('https://github.com/Jackett/Jackett')}>Jackett</button> or another Torznab feed, paste its feed URL (Jackett: <strong>Copy Torznab Feed</strong>, or the “All” feed) and the API key at the top of Jackett’s page.</p>
          <div class="ixform">
            <input class="input" bind:value={ixName} placeholder="Name (optional)" spellcheck="false" />
            <input class="input mono" bind:value={ixUrl} placeholder="Torznab URL" spellcheck="false" />
            <input class="input mono" type="password" bind:value={ixKey} placeholder="API key" spellcheck="false" autocomplete="off" />
            <button class="btn primary" disabled={!ixUrl.trim() || ixBusy} onclick={addIndexer}>{#if ixBusy}<span class="spinner"></span>{/if}Test and add</button>
          </div>
        </div>

      {:else if section.id === 'browser'}
        <div class="group">
          <SettingRow title="Accept downloads from the browser" description="The Haul extension sends downloads here. Only this PC can reach it.">
            <Switch checked={s.bridgeEnabled} label="Browser downloads" onchange={(v) => set('bridgeEnabled', v)} />
          </SettingRow>
          <SettingRow title="Port" description="Change only if another app uses it (then set the same port in the extension’s options).">
            <input class="input n" type="number" min="1024" max="65535" value={s.bridgePort} onchange={(e) => set('bridgePort', num(e, 1024, 65535))} />
          </SettingRow>
        </div>
        <h3>Chrome, Edge or Drift</h3>
        <div class="group steps">
          <ol>
            <li>Click <button class="btn sm" onclick={() => api.openExtensionFolder()}><Icon name="folder" size={14} />Open extension folder</button></li>
            <li>Open <span class="mono">chrome://extensions</span> (Edge: <span class="mono">edge://extensions</span>).</li>
            <li>Turn on <strong>Developer mode</strong>, click <strong>Load unpacked</strong> and choose that folder.</li>
          </ol>
        </div>
        <h3>Firefox</h3>
        <div class="group steps">
          <ol>
            <li>Click <button class="btn sm" onclick={() => api.openExtensionFolder(true)}><Icon name="folder" size={14} />Open Firefox extension folder</button></li>
            <li>Open <span class="mono">about:debugging#/runtime/this-firefox</span> and click <strong>Load Temporary Add-on…</strong></li>
            <li>Choose <span class="mono">manifest.json</span> in that folder, then click the Haul icon once and allow it to see downloads.</li>
          </ol>
          <p class="faint small pad-l">Firefox forgets temporary add-ons when it closes. For a permanent install use Firefox Developer Edition or ESR with signing turned off, or the zip from the GitHub release.</p>
        </div>
        <p class="faint small">Downloads now go to Haul. Right-click a link for <strong>Download with Haul</strong>, or a page for <strong>Download video with Haul</strong>; video pages also get a small button. Hold <span class="kbd">Alt</span> while clicking a link to let the browser handle it.</p>

      {:else if section.id === 'appearance'}
        <h3>Theme</h3>
        <div class="themes">
          {#each allThemes() as t (t.id)}
            <button class="theme" class:on={s.theme === t.id} style={preview(t)} onclick={() => set('theme', t.id)} title={t.description}>
              <span class="pv">
                <span class="pv-side"></span>
                <span class="pv-main"><span class="pv-row"></span><span class="pv-row short"></span><span class="pv-bar"></span></span>
              </span>
              <span class="tn">{t.name}{#if t.backdrop !== 'none'}<span class="faint">&nbsp;· {t.backdrop === 'acrylic' ? 'Acrylic' : 'Mica'}</span>{/if}</span>
            </button>
          {/each}
        </div>
        <div class="row-actions">
          <button class="btn sm" onclick={() => api.openThemesFolder()}><Icon name="folder" size={14} />Open folder</button>
          <button class="btn sm" onclick={() => reloadThemes().then(() => toast(`${themes.custom.length} theme file${themes.custom.length === 1 ? '' : 's'} loaded`, 'success'))}><Icon name="refresh" size={14} />Reload</button>
          <button class="btn sm" onclick={saveThemeFile}><Icon name="copy" size={14} />Save as file</button>
          <span class="faint small">Theme files from Haul’s and Hangar’s themes folders.</span>
        </div>
        {#each themes.errors as err (err)}<p class="bad small">{err}</p>{/each}
        {#if s.theme !== 'midnight' && s.theme !== 'graphite' && s.theme !== 'paper' && !themes.backdrop}
          <p class="faint small">Mica and Acrylic backdrops need Windows 11 22H2 or later; here the theme’s solid colors are used.</p>
        {/if}

        <h3>Accent color</h3>
        <div class="accents">
          <button class="acc wide" class:on={s.accent === 'theme'} onclick={() => set('accent', 'theme')}>Theme’s own</button>
          <button class="acc wide" class:on={s.accent === 'windows'} onclick={() => set('accent', 'windows')}>
            <span class="sw" style="background: {themes.systemAccent ?? '#4f8cff'}"></span>Windows
          </button>
          {#each ACCENT_SWATCHES as c (c)}
            <button class="acc" class:on={s.accent.toLowerCase() === c} style="--c: {c}" onclick={() => set('accent', c)} aria-label="Accent {c}"><span class="sw"></span></button>
          {/each}
          <label class="acc wide custom" class:on={s.accent.startsWith('#') && !ACCENT_SWATCHES.includes(s.accent.toLowerCase())}>
            <input type="color" value={s.accent.startsWith('#') ? s.accent : '#4f8cff'} onchange={(e) => set('accent', (e.currentTarget as HTMLInputElement).value)} />Custom…
          </label>
        </div>
        <p class="faint small">Accents stay readable: dark accents are lightened on dark themes, bright ones darkened on light themes.</p>

        <h3>Layout</h3>
        <div class="group">
          <SettingRow title="Speeds in Mbps" description="Show speeds in megabits per second, the unit internet plans use (1 MB/s = 8 Mbps), instead of megabytes per second.">
            <Switch checked={s.speedBits} label="Speeds in Mbps" onchange={(v) => set('speedBits', v)} />
          </SettingRow>
          <SettingRow title="Compact rows" description="Fit more downloads on screen.">
            <Switch checked={s.compactRows} label="Compact rows" onchange={(v) => set('compactRows', v)} />
          </SettingRow>
          <SettingRow title="Windows title bar" description="Use the standard title bar instead of Haul’s own.">
            <Switch checked={s.nativeTitleBar} label="Windows title bar" onchange={(v) => set('nativeTitleBar', v)} />
          </SettingRow>
        </div>
        <h3>Download tracking style</h3>
        <div class="pstyles" role="radiogroup" aria-label="Progress style">
          {#each [['glow', 'Glow', 'Soft light with a sheen'], ['classic', 'Classic', 'Flat and calm'], ['striped', 'Striped', 'Moving candy stripes'], ['aurora', 'Aurora', 'Shifting colours'], ['blocks', 'Blocks', 'Retro segments'], ['minimal', 'Minimal', 'A hairline'], ['neon', 'Neon', 'Bright, pulsing outline']] as [id, name, hint], n (id)}
            <button class="ps" class:on={(s.progressStyle || 'glow') === id} role="radio" aria-checked={(s.progressStyle || 'glow') === id} onclick={() => set('progressStyle', id)} data-progress={id}>
              <span class="pgv run">
                <span class="bar"><span class="fill" style="width: {[72, 58, 64, 81, 47, 69, 55][n]}%"></span></span>
                <span class="bar"><span class="fill" style="width: {[38, 87, 29, 52, 76, 33, 90][n]}%"></span></span>
              </span>
              <span class="pvg"><SpeedGraph data={SAMPLE} height={34} variant={id} dot={false} /></span>
              <span class="pvl"><span>{name}</span><small>{hint}</small></span>
            </button>
          {/each}
        </div>


      {:else if section.id === 'about'}
        <div class="about group">
          <svg viewBox="0 0 24 24" width="44" height="44" aria-hidden="true">
            <rect width="24" height="24" rx="6" fill="var(--surface-strong)" />
            <path d="M12 5v8M8.7 10l3.3 3.3L15.3 10" fill="none" stroke="var(--accent)" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" />
            <path d="M6.5 14.5v2.3a1.7 1.7 0 0 0 1.7 1.7h7.6a1.7 1.7 0 0 0 1.7-1.7v-2.3" fill="none" stroke="var(--text)" stroke-width="1.9" stroke-linecap="round" />
          </svg>
          <div>
            <div class="appname">Haul {app.version}</div>
            <div class="muted small">Downloads and torrents. Part of the larosa023 suite.{app.portable ? ' Portable mode.' : ''}</div>
            <div class="small"><button class="link" onclick={() => api.openDataFolder()}>Open data folder</button></div>
          </div>
        </div>
        <div class="group">
          <SettingRow title="Updates" description={app.update?.status === 'ready' ? `Haul ${app.update.version} is ready to install.` : app.update?.status === 'available' ? `Haul ${app.update.version} is available.` : app.update?.status === 'downloading' ? `Downloading ${app.update.version}… ${Math.round((app.update.progress ?? 0) * 100)}%` : app.update?.status === 'error' ? app.update.error : app.update?.status === 'none' ? 'Haul is up to date.' : 'Checked on launch and every few hours.'}>
            {#if app.update?.status === 'ready'}
              <button class="btn sm primary" onclick={() => api.applyUpdate(true).catch((e) => toast(errorText(e), 'error'))}>Restart now</button>
            {:else}
              <button class="btn sm" disabled={checking || app.update?.status === 'downloading'} onclick={checkNow}>{#if checking}<span class="spinner"></span>{/if}{app.update?.status === 'available' ? 'Download' : 'Check for updates now'}</button>
            {/if}
          </SettingRow>
          <SettingRow title="Update automatically" description="Download updates in the background and restart into them once nothing is downloading and the window isn’t open.">
            <Switch checked={s.autoUpdate} label="Update automatically" onchange={(v) => set('autoUpdate', v)} />
          </SettingRow>
          <SettingRow title="Channel" description="Beta gets new versions first (they may have rough edges).">
            <div class="seg">
              <button class:on={s.channel === 'stable'} onclick={() => set('channel', 'stable')}>Stable</button>
              <button class:on={s.channel === 'beta'} onclick={() => set('channel', 'beta')}>Beta</button>
            </div>
          </SettingRow>
          {#if s.skipVersion}
            <SettingRow title="Skipped version" description={`Haul ${s.skipVersion} didn’t start properly, so it was rolled back and is skipped.`}>
              <button class="btn sm ghost" onclick={() => set('skipVersion', '')}>Try it again</button>
            </SettingRow>
          {/if}
        </div>
        <h3>Keyboard</h3>
        <div class="group keys">
          {#each [['Ctrl+N', 'New download'], ['Ctrl+O', 'Open .torrent file'], ['Ctrl+V', 'Paste a link to download it'], ['Space', 'Pause / resume'], ['Del', 'Remove (with an option to delete files)'], ['Enter / Shift+Enter', 'Open / show in folder'], ['↑ ↓, Shift for more', 'Select'], ['Ctrl+A', 'Select all'], ['Alt+↑ / Alt+↓', 'Move in the queue (Ctrl for top / bottom)'], ['Ctrl+F', 'Search'], ['F2', 'Details panel'], ['Ctrl+,', 'Settings'], ['Ctrl+K', 'Search torrents'], [s.miniHotkey || '—', 'Mini window (anywhere in Windows)'], ['Esc', 'Back / clear']] as [k, d] (k)}
            <div class="key"><span class="kbd">{k}</span><span>{d}</span></div>
          {/each}
        </div>
      {/if}
    {/key}
  </div>
</div>

<style>
  .settings { flex: 1; min-height: 0; display: grid; grid-template-columns: 210px minmax(0, 760px); gap: 28px; padding: 6px 24px 0 18px; overflow: hidden; }
  .side { display: flex; flex-direction: column; gap: 1px; overflow-y: auto; padding-bottom: 20px; }
  .cat { position: relative; display: flex; align-items: center; gap: 11px; height: 36px; padding: 0 10px; border: 0; border-radius: 6px; background: none; color: var(--text-2); cursor: pointer; text-align: left; font-weight: 500; }
  .cat:hover { background: var(--surface-hover); color: var(--text); }
  .cat.on { background: var(--surface); color: var(--text); }
  .cat.on :global(svg) { color: var(--accent); }
  .badge { width: 8px; height: 8px; border-radius: 50%; background: var(--accent); margin-left: auto; }
  .content { overflow-y: auto; padding: 4px 6px 40px 2px; animation: fade 160ms var(--ease); user-select: text; }
  @keyframes fade { from { opacity: 0; transform: translateY(3px); } }
  header { margin-bottom: 14px; }
  header h2 { margin: 0; font: 600 21px var(--font-display); }
  header p { margin: 3px 0 0; font-size: 13px; }
  h3 { font: 600 13.5px var(--font); margin: 20px 0 8px; }
  .group { background: var(--surface); border: 1px solid var(--line); border-radius: 8px; margin-bottom: 10px; overflow: hidden; }
  .group :global(.srow + .srow) { border-top-color: var(--line); }
  .input.n { width: 96px; }
  .input.n2 { width: 120px; }
  .short-sel { width: 110px; }
  .pad-l { padding: 0 16px 12px; margin: 0; }
  .input.t { width: 120px; }
  .select { width: 200px; }
  .select.wide { width: 260px; }
  .unit { display: flex; align-items: center; gap: 8px; color: var(--text-2); font-size: 12.5px; }
  .pathrow { display: flex; gap: 6px; align-items: center; }
  .ok { display: inline-flex; align-items: center; gap: 6px; color: var(--ok); font-weight: 600; font-size: 12.5px; }
  .bad { display: inline-flex; align-items: center; gap: 6px; color: var(--danger); font-size: 12.5px; }
  .small { font-size: 12.5px; }
  .link { border: 0; background: none; color: var(--accent); cursor: pointer; padding: 0; font-size: inherit; }
  .link:hover { text-decoration: underline; }
  .grow { flex: 1; }
  .frow { display: grid; grid-template-columns: 90px 1fr auto auto; gap: 8px; align-items: center; padding: 7px 12px; }
  .frow + .frow { border-top: 1px solid var(--line); }
  .fcat { font-weight: 600; font-size: 12.5px; }
  .rules { font-size: 12.5px; }
  .rhead, .rrow { display: grid; grid-template-columns: 18px 110px 1fr 64px 64px 120px 90px; gap: 6px; align-items: center; padding: 6px 10px; }
  .rhead { color: var(--text-3); font-weight: 600; font-size: 11.5px; padding-top: 8px; }
  .rrow { border-top: 1px solid var(--line); }
  .rrow .input, .rrow .select { height: 28px; font-size: 12.5px; width: 100%; }
  .rrow.off { opacity: 0.55; }
  .ract { display: flex; justify-content: flex-end; }
  .icon-btn.s { width: 26px; height: 26px; }
  .rfoot { display: flex; gap: 8px; padding: 8px 10px; border-top: 1px solid var(--line); }
  .days { display: flex; gap: 4px; }
  .days button { border: 1px solid var(--line); background: var(--surface); border-radius: 6px; padding: 4px 8px; cursor: pointer; color: var(--text-2); font-size: 12px; font-weight: 600; }
  .days button.on { background: var(--accent); color: var(--on-accent); border-color: transparent; }
  .status-card { display: flex; align-items: center; gap: 12px; padding: 12px 14px; border-radius: 8px; background: var(--surface); border: 1px solid var(--line); }
  .status-card .dot { width: 10px; height: 10px; border-radius: 50%; background: var(--warn); flex-shrink: 0; }
  .status-card .dot.good { background: var(--ok); box-shadow: 0 0 8px var(--ok); }
  .connect { padding: 14px 16px; font-size: 13px; }
  .svcs { margin-bottom: 12px; }
  .svcs button { display: inline-flex; align-items: center; gap: 6px; }
  .saved { width: 6px; height: 6px; border-radius: 50%; background: var(--text-3); }
  .saved.inuse { background: var(--ok); box-shadow: 0 0 6px var(--ok); }
  .ixform { display: grid; grid-template-columns: 1fr 2fr 1.2fr auto; gap: 6px; }
  .pstyles { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 8px; margin-bottom: 8px; }
  .ps { display: flex; flex-direction: column; gap: 9px; padding: 12px 12px 10px; border-radius: 10px; border: 1px solid var(--line); background: var(--surface); cursor: pointer; text-align: left; color: var(--text); transition: border-color var(--dur), background var(--dur); }
  .ps:hover { background: var(--surface-hover); }
  .ps.on { border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent) inset; }
  .ps .pvg { position: relative; display: block; margin: 2px 0 -2px; }
  .ps .pvl { display: flex; flex-direction: column; gap: 1px; font-size: 12.5px; font-weight: 600; }
  .ps .pvl small { color: var(--text-3); font-size: 11px; font-weight: 400; }
  .connect p { margin: 0 0 8px; }
  .connect ol { margin: 0 0 12px; padding-left: 20px; line-height: 1.7; }
  .connect .pathrow .input { flex: 1; }
  .steps ol { margin: 0; padding: 12px 16px 12px 34px; display: flex; flex-direction: column; gap: 10px; font-size: 13px; line-height: 1.6; }
  .themes { display: grid; grid-template-columns: repeat(auto-fill, minmax(132px, 1fr)); gap: 10px; }
  .theme { display: flex; flex-direction: column; gap: 6px; padding: 6px; border-radius: 8px; border: 2px solid transparent; background: none; cursor: pointer; text-align: left; color: var(--text); }
  .theme:hover { background: var(--surface-hover); }
  .theme.on { border-color: var(--accent); }
  .pv { display: flex; height: 70px; border-radius: 6px; overflow: hidden; background: var(--p-bg); border: 1px solid var(--line); }
  .pv-side { width: 26%; background: var(--p-side); }
  .pv-main { flex: 1; padding: 8px; display: flex; flex-direction: column; gap: 5px; }
  .pv-row { height: 9px; border-radius: 3px; background: var(--p-surface); }
  .pv-row.short { width: 60%; }
  .pv-bar { margin-top: auto; height: 5px; width: 70%; border-radius: 3px; background: var(--p-accent); }
  .tn { font-size: 12px; font-weight: 600; padding: 0 2px; }
  .row-actions { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin-top: 10px; }
  .accents { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  .acc { height: 32px; min-width: 32px; display: inline-flex; align-items: center; justify-content: center; gap: 7px; padding: 0 4px; border-radius: 16px; border: 2px solid transparent; background: var(--surface); cursor: pointer; font-size: 12.5px; color: var(--text); }
  .acc.wide { padding: 0 12px; }
  .acc.on { border-color: var(--accent); }
  .acc .sw { width: 20px; height: 20px; border-radius: 50%; background: var(--c); display: inline-block; }
  .acc.wide .sw { width: 14px; height: 14px; }
  .custom input { width: 18px; height: 18px; border: 0; padding: 0; background: none; cursor: pointer; }
  .about { display: flex; align-items: center; gap: 16px; padding: 16px; }
  .appname { font: 700 19px var(--font-display); }
  .keys { display: grid; grid-template-columns: 1fr 1fr; gap: 8px 18px; padding: 14px; }
  .key { display: flex; align-items: center; gap: 10px; font-size: 12.5px; }
  .key .kbd { min-width: 88px; text-align: center; }
  @media (max-width: 900px) { .settings { grid-template-columns: 1fr; } .side { flex-direction: row; flex-wrap: wrap; } }
</style>
