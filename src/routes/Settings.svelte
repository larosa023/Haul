<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '../lib/components/Icon.svelte';
  import SettingRow from '../lib/components/SettingRow.svelte';
  import Switch from '../lib/components/Switch.svelte';
  import { api, isTauri, type Adapter, type Rule, type Settings } from '../lib/api';
  import { app, ui, saveSettings, toast, errorText } from '../lib/state.svelte';
  import { THEMES, ACCENT_SWATCHES, cssVars, effectiveTheme, resolveAccent, themeToJson, type ThemeDef } from '../lib/themecore.js';
  import { themes, allThemes, reloadThemes, findTheme } from '../lib/theme.svelte';

  const SECTIONS = [
    { id: 'general', label: 'General', icon: 'settings', blurb: 'Startup, the tray and new downloads' },
    { id: 'downloads', label: 'Downloads', icon: 'download', blurb: 'Folders, connections and the queue' },
    { id: 'sorting', label: 'Sorting', icon: 'sort', blurb: 'Where finished files go, and extracting archives' },
    { id: 'speed', label: 'Speed & schedule', icon: 'gauge', blurb: 'Limits, and slower speeds at set times' },
    { id: 'torrents', label: 'Torrents', icon: 'magnet', blurb: 'Seeding, peers, privacy and ports' },
    { id: 'network', label: 'Network adapter', icon: 'network', blurb: 'Keep torrents on your VPN' },
    { id: 'browser', label: 'Browser', icon: 'puzzle', blurb: 'Send downloads from Chrome, Edge or Drift' },
    { id: 'appearance', label: 'Appearance', icon: 'brush', blurb: 'Themes, accent color and layout' },
    { id: 'about', label: 'About & updates', icon: 'refresh', blurb: 'Version, updates and shortcuts' },
  ];
  const DAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

  const s = $derived(app.settings!);
  const section = $derived(SECTIONS.find((x) => x.id === ui.settingsTab) ?? SECTIONS[0]);

  let adapters: Adapter[] = $state([]);
  let handlers = $state({ magnet: false, torrent: false });
  let checking = $state(false);

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
          <SettingRow title="Connections per download" description="Big files are split into this many parts that download at once (when the server allows it).">
            <input class="input n" type="number" min="1" max="32" value={s.connections} onchange={(e) => set('connections', num(e, 1, 32))} />
          </SettingRow>
          <SettingRow title="Retries" description="How many times a failed connection or download is retried, with a growing pause between tries.">
            <input class="input n" type="number" min="0" max="50" value={s.maxRetries} onchange={(e) => set('maxRetries', num(e, 0, 50))} />
          </SettingRow>
          <SettingRow title="Downloads at once" description="Others wait in the queue (top of the list first, High priority before Normal). 0 = no limit.">
            <input class="input n" type="number" min="0" max="100" value={s.maxActive} onchange={(e) => set('maxActive', num(e, 0, 100))} />
          </SettingRow>
          <SettingRow title="User agent" description="What Haul tells servers it is. Leave empty for the default; downloads from the browser use the browser’s." wide>
            <input class="input mono" value={s.userAgent} placeholder="Default" onchange={(e) => set('userAgent', (e.currentTarget as HTMLInputElement).value)} spellcheck="false" />
          </SettingRow>
        </div>

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

      {:else if section.id === 'browser'}
        <div class="group">
          <SettingRow title="Accept downloads from the browser" description="The Haul extension sends downloads here. Only this PC can reach it.">
            <Switch checked={s.bridgeEnabled} label="Browser downloads" onchange={(v) => set('bridgeEnabled', v)} />
          </SettingRow>
          <SettingRow title="Port" description="Change only if another app uses it (then set the same port in the extension’s options).">
            <input class="input n" type="number" min="1024" max="65535" value={s.bridgePort} onchange={(e) => set('bridgePort', num(e, 1024, 65535))} />
          </SettingRow>
        </div>
        <h3>Install the extension</h3>
        <div class="group steps">
          <ol>
            <li>Click <button class="btn sm" onclick={() => api.openExtensionFolder()}><Icon name="folder" size={14} />Open extension folder</button></li>
            <li>In Chrome, Edge or Drift open <span class="mono">chrome://extensions</span> (Edge: <span class="mono">edge://extensions</span>).</li>
            <li>Turn on <strong>Developer mode</strong>, click <strong>Load unpacked</strong> and choose that folder.</li>
            <li>Downloads now go to Haul. Right-click a link for <strong>Download with Haul</strong>; hold <span class="kbd">Alt</span> while clicking to let the browser handle one itself.</li>
          </ol>
        </div>

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
          <SettingRow title="Compact rows" description="Fit more downloads on screen.">
            <Switch checked={s.compactRows} label="Compact rows" onchange={(v) => set('compactRows', v)} />
          </SettingRow>
          <SettingRow title="Windows title bar" description="Use the standard title bar instead of Haul’s own.">
            <Switch checked={s.nativeTitleBar} label="Windows title bar" onchange={(v) => set('nativeTitleBar', v)} />
          </SettingRow>
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
          {#each [['Ctrl+N', 'New download'], ['Ctrl+O', 'Open .torrent file'], ['Ctrl+V', 'Paste a link to download it'], ['Space', 'Pause / resume'], ['Del', 'Remove (with an option to delete files)'], ['Enter / Shift+Enter', 'Open / show in folder'], ['↑ ↓, Shift for more', 'Select'], ['Ctrl+A', 'Select all'], ['Alt+↑ / Alt+↓', 'Move in the queue (Ctrl for top / bottom)'], ['Ctrl+F', 'Search'], ['F2', 'Details panel'], ['Ctrl+,', 'Settings'], ['Esc', 'Back / clear']] as [k, d] (k)}
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
