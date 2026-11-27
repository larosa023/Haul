<script lang="ts">
  import Icon from '../lib/components/Icon.svelte';
  import { api, type SearchHit } from '../lib/api';
  import { app, ui, toast, errorText, selectOnly, findDuplicate } from '../lib/state.svelte';
  import { bytes, debridName } from '../lib/format.js';

  // Kept between visits, so going to the list and back keeps the results.
  const memo = (globalThis as unknown as { __haulSearch?: { q: string; cat: string; hits: SearchHit[]; errors: [string, string][]; searched: boolean } }).__haulSearch ??= {
    q: '',
    cat: '',
    hits: [],
    errors: [],
    searched: false,
  };

  let q = $state(memo.q);
  let cat = $state(memo.cat);
  let hits = $state<SearchHit[]>(memo.hits);
  let errors = $state<[string, string][]>(memo.errors);
  let searched = $state(memo.searched);
  let loading = $state(false);
  let sort = $state<'seeders' | 'size' | 'published' | 'title'>('seeders');
  let desc = $state(true);
  let filter = $state('');
  let adding = $state(new Set<string>());

  const CATS = [
    ['', 'All'],
    ['movies', 'Movies'],
    ['tv', 'TV'],
    ['music', 'Music'],
    ['software', 'Software'],
    ['games', 'Games'],
    ['books', 'Books'],
  ];
  const s = $derived(app.settings!);
  const sources = $derived((s.searchArchive ? 1 : 0) + s.searchIndexers.filter((i) => i.enabled).length);
  const debrid = $derived(s.debridEnabled && !!s.debridKeys?.[s.debridService] && s.debridTorrents !== 'off');
  const shown = $derived.by(() => {
    const f = filter.trim().toLowerCase();
    const list = f ? hits.filter((h) => h.title.toLowerCase().includes(f) || h.source.toLowerCase().includes(f)) : hits.slice();
    const k = sort;
    list.sort((a, b) => {
      const v = k === 'title' ? a.title.localeCompare(b.title) : k === 'seeders' ? Math.max(a.seeders, -1) - Math.max(b.seeders, -1) : (a[k] as number) - (b[k] as number);
      return desc ? -v : v;
    });
    return list;
  });

  async function run() {
    if (!q.trim() || loading) return;
    loading = true;
    try {
      const r = await api.searchTorrents(q.trim(), cat);
      hits = r.hits;
      errors = r.errors;
      searched = true;
      Object.assign(memo, { q, cat, hits: r.hits, errors: r.errors, searched: true });
    } catch (e) {
      toast(errorText(e), 'error');
    } finally {
      loading = false;
    }
  }

  function by(k: typeof sort) {
    if (sort === k) desc = !desc;
    else {
      sort = k;
      desc = k !== 'title';
    }
  }

  async function add(h: SearchHit, viaDebrid?: boolean) {
    adding = new Set(adding).add(h.link);
    try {
      const r = await api.searchAdd(h.link, viaDebrid);
      if (!r.existing) toast(`Added ${h.title}`, 'success');
      selectOnly(r.id);
    } catch (e) {
      toast(errorText(e), 'error', 8000);
    } finally {
      const n = new Set(adding);
      n.delete(h.link);
      adding = n;
    }
  }

  function age(t: number) {
    if (!t) return '';
    const d = (Date.now() / 1000 - t) / 86400;
    if (d < 1) return 'today';
    if (d < 30) return `${Math.floor(d)}d`;
    if (d < 365) return `${Math.floor(d / 30)}mo`;
    return `${(d / 365).toFixed(d < 3650 ? 1 : 0)}y`;
  }

  const health = (h: SearchHit) => (h.seeders < 0 ? 'na' : h.seeders >= 50 ? 'good' : h.seeders >= 5 ? 'ok' : 'low');
</script>

<section class="search">
  <div class="top">
    <form class="bar" onsubmit={(e) => { e.preventDefault(); run(); }}>
      <Icon name="search" size={17} />
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={q} placeholder="Search torrents" spellcheck="false" autofocus />
      <button class="btn primary" type="submit" disabled={!q.trim() || loading || !sources}>{#if loading}<span class="spinner"></span>{/if}Search</button>
    </form>
    <div class="cats">
      {#each CATS as [id, label] (id)}
        <button class="cc" class:on={cat === id} onclick={() => { cat = id; if (q.trim()) run(); }}>{label}</button>
      {/each}
      <span class="grow"></span>
      <button class="link small" onclick={() => { ui.view = 'settings'; ui.settingsTab = 'search'; }}><Icon name="settings" size={13} /> {sources} source{sources === 1 ? '' : 's'}</button>
    </div>
  </div>

  {#if errors.length}
    <div class="errs">
      {#each errors as [name, e] (name)}<span><Icon name="alert" size={13} /> {name} {e}</span>{/each}
    </div>
  {/if}

  {#if hits.length}
    <div class="tools">
      <input class="input" bind:value={filter} placeholder="Filter results" spellcheck="false" />
      <span class="muted small">{shown.length} result{shown.length === 1 ? '' : 's'}</span>
    </div>
    <div class="table" role="table">
      <div class="th" role="row">
        <button class="c-title" onclick={() => by('title')}>Name {#if sort === 'title'}<Icon name={desc ? 'down' : 'up'} size={12} />{/if}</button>
        <button class="c-size" onclick={() => by('size')}>Size {#if sort === 'size'}<Icon name={desc ? 'down' : 'up'} size={12} />{/if}</button>
        <button class="c-seed" onclick={() => by('seeders')}>Seeds {#if sort === 'seeders'}<Icon name={desc ? 'down' : 'up'} size={12} />{/if}</button>
        <button class="c-age" onclick={() => by('published')}>Age {#if sort === 'published'}<Icon name={desc ? 'down' : 'up'} size={12} />{/if}</button>
        <span class="c-act"></span>
      </div>
      <div class="rows">
        {#each shown as h (h.link + h.source)}
          {@const dup = findDuplicate(h.link)}
          <div class="tr" role="row">
            <div class="c-title">
              <span class="t" title={h.title}>{h.title}</span>
              <span class="meta">
                <span class="src">{h.source}</span>
                {#if h.category}<span>{h.category}</span>{/if}
                {#if h.downloads}<span>{h.downloads.toLocaleString()} downloads</span>{/if}
                {#if dup}<span class="have"><Icon name="check" size={11} stroke={2.6} /> In your list</span>{/if}
              </span>
            </div>
            <span class="c-size">{h.size ? bytes(h.size) : '—'}</span>
            <span class="c-seed {health(h)}">{#if h.seeders >= 0}<span class="dot"></span>{h.seeders.toLocaleString()}<small> / {Math.max(h.leechers, 0)}</small>{:else}<small>—</small>{/if}</span>
            <span class="c-age">{age(h.published)}</span>
            <span class="c-act">
              {#if h.infoUrl}<button class="icon-btn" title="Open its page" onclick={() => api.openLink(h.infoUrl)}><Icon name="open" size={15} /></button>{/if}
              {#if debrid}<button class="icon-btn" title="Download through {debridName(s.debridService)}" disabled={adding.has(h.link)} onclick={() => add(h, true)}><Icon name="cloud" size={15} /></button>{/if}
              <button class="btn sm primary" disabled={adding.has(h.link)} onclick={() => add(h, debrid ? false : undefined)}>{#if adding.has(h.link)}<span class="spinner"></span>{:else}<Icon name="download" size={14} />{/if}Get</button>
            </span>
          </div>
        {/each}
      </div>
    </div>
  {:else if loading}
    <div class="empty"><span class="spinner big"></span><span>Asking {sources} source{sources === 1 ? '' : 's'}…</span></div>
  {:else if searched}
    <div class="empty"><Icon name="search" size={30} /><b>Nothing found</b><span class="muted">Try other words, or a different category.</span></div>
  {:else}
    <div class="empty">
      <Icon name="search" size={34} />
      <b>Search for torrents</b>
      <span class="muted">Haul searches the Internet Archive{s.searchIndexers.length ? ` and your ${s.searchIndexers.length} indexer${s.searchIndexers.length === 1 ? '' : 's'}` : ''} at once and sorts by seeders.</span>
      {#if !s.searchIndexers.length}<button class="btn sm" onclick={() => { ui.view = 'settings'; ui.settingsTab = 'search'; }}>Add Prowlarr or Jackett indexers</button>{/if}
    </div>
  {/if}
</section>

<style>
  .search { flex: 1; min-height: 0; display: flex; flex-direction: column; gap: 10px; padding: 4px 14px 14px; }
  .top { display: flex; flex-direction: column; gap: 10px; }
  .bar { display: flex; align-items: center; gap: 10px; padding: 6px 6px 6px 14px; border-radius: 12px; border: 1px solid var(--line); background: var(--surface); }
  .bar:focus-within { border-color: var(--accent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent); }
  .bar :global(svg) { color: var(--text-3); flex-shrink: 0; }
  .bar input { flex: 1; border: 0; background: none; outline: none; color: var(--text); font: 500 15px var(--font-display); min-width: 0; }
  .cats { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .cc { border: 1px solid var(--line); background: var(--surface); padding: 4px 12px; border-radius: 14px; cursor: pointer; font-size: 12.5px; color: var(--text-2); }
  .cc.on { background: var(--accent); color: var(--on-accent); border-color: transparent; }
  .grow { flex: 1; }
  .link { display: inline-flex; align-items: center; gap: 5px; border: 0; background: none; color: var(--text-2); cursor: pointer; font-size: 12.5px; }
  .link:hover { color: var(--text); }
  .errs { display: flex; flex-wrap: wrap; gap: 4px 14px; font-size: 12px; color: var(--warn); }
  .errs span { display: inline-flex; align-items: center; gap: 4px; }
  .tools { display: flex; align-items: center; gap: 10px; }
  .tools .input { width: 260px; }
  .table { flex: 1; min-height: 0; display: flex; flex-direction: column; border: 1px solid var(--line); border-radius: 10px; overflow: hidden; background: var(--surface-1, transparent); }
  .th, .tr { display: grid; grid-template-columns: minmax(0, 1fr) 90px 110px 64px 150px; align-items: center; gap: 10px; padding: 0 12px; }
  .th { height: 34px; border-bottom: 1px solid var(--line); font-size: 12px; color: var(--text-3); }
  .th button { justify-content: flex-start; display: inline-flex; align-items: center; gap: 3px; border: 0; background: none; color: inherit; font: inherit; cursor: pointer; padding: 0; text-align: left; }
  .th button:hover { color: var(--text); }
  .th .c-title { flex-direction: row; align-items: center; padding: 0; }
  .rows { flex: 1; overflow: auto; }
  .tr { min-height: 50px; border-bottom: 1px solid var(--line); content-visibility: auto; contain-intrinsic-size: auto 50px; }
  .tr:hover { background: var(--surface-hover); }
  .c-title { display: flex; flex-direction: column; gap: 3px; min-width: 0; padding: 7px 0; }
  .t { font-weight: 600; font-size: 13px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .meta { display: flex; gap: 10px; font-size: 11.5px; color: var(--text-3); white-space: nowrap; overflow: hidden; }
  .src { color: var(--text-2); }
  .have { display: inline-flex; align-items: center; gap: 3px; color: var(--ok); }
  .c-size, .c-age { font-size: 12.5px; color: var(--text-2); font-variant-numeric: tabular-nums; }
  .c-seed { display: flex; align-items: center; gap: 6px; font-size: 12.5px; font-weight: 600; font-variant-numeric: tabular-nums; }
  .c-seed small { color: var(--text-3); font-weight: 400; }
  .c-seed .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--text-3); }
  .c-seed.good .dot { background: var(--ok); box-shadow: 0 0 6px var(--ok); }
  .c-seed.ok .dot { background: var(--warn); }
  .c-seed.low .dot { background: var(--danger); }
  .c-act { display: flex; align-items: center; justify-content: flex-end; gap: 4px; }
  .empty { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px; color: var(--text-3); text-align: center; font-size: 13px; padding: 20px; }
  .empty b { color: var(--text); font-size: 15px; }
  .empty .muted { max-width: 420px; }
  .spinner.big { width: 22px; height: 22px; }
</style>
