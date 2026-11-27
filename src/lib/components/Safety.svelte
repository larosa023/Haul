<script lang="ts">
  // A safety grade (A–F) with the reasons behind it.
  import Icon from './Icon.svelte';
  import type { SafetyReport } from '../api';

  let { report, open = false, compact = false, scanning = false, onrescan }: { report: SafetyReport; open?: boolean; compact?: boolean; scanning?: boolean; onrescan?: () => void } = $props();
  // svelte-ignore state_referenced_locally
  let expanded = $state(open);

  const tone = $derived(report.grade === 'A' || report.grade === 'B' ? 'ok' : report.grade === 'C' ? 'mid' : 'bad');
  const verdict = $derived(
    { A: 'Looks safe', B: 'Probably safe', C: 'Be careful', D: 'Risky', F: 'Likely dangerous' }[report.grade] ?? '',
  );
  const ICON: Record<string, string> = { good: 'check', info: 'info', warn: 'alert', bad: 'x' };
  const stage = $derived(report.stage === 'file' ? (report.defender === 'clean' ? 'File checked · Defender scanned' : 'File checked') : 'Link checked · the file is checked when it finishes');
</script>

<div class="safety {tone}" class:compact>
  <button class="head" onclick={() => (expanded = !expanded)} aria-expanded={expanded} title="Why this score?">
    <span class="grade">{report.grade}</span>
    <span class="txt">
      <span class="verdict">{verdict} <span class="score">{report.score}/100</span></span>
      {#if !compact}<span class="stage">{stage}</span>{/if}
    </span>
    <span class="why">{expanded ? 'Hide' : 'Why?'}<Icon name={expanded ? 'up' : 'down'} size={13} /></span>
  </button>
  {#if expanded}
    <ul class="list">
      {#each report.findings as f, i (i)}
        <li class={f.level}><span class="ic"><Icon name={ICON[f.level]} size={13} stroke={2.4} /></span><span class="ft">{f.text}</span>{#if f.points}<span class="pts">−{f.points}</span>{/if}</li>
      {/each}
    </ul>
    {#if onrescan}
      <button class="btn sm ghost again" disabled={scanning} onclick={onrescan}>{#if scanning}<span class="spinner"></span>Checking…{:else}<Icon name="refresh" size={13} />Check again{/if}</button>
    {/if}
  {/if}
</div>

<style>
  .safety { --c: var(--ok); border: 1px solid color-mix(in srgb, var(--c) 35%, var(--line)); border-radius: 9px; background: color-mix(in srgb, var(--c) 7%, transparent); user-select: none; }
  .safety.mid { --c: var(--warn); }
  .safety.bad { --c: var(--danger); }
  .head { display: flex; align-items: center; gap: 10px; width: 100%; padding: 7px 10px; border: 0; background: none; cursor: pointer; text-align: left; color: var(--text); }
  .grade { width: 30px; height: 30px; display: grid; place-items: center; border-radius: 8px; flex-shrink: 0; font: 700 17px var(--font-display); color: #fff; background: var(--c); box-shadow: 0 0 12px color-mix(in srgb, var(--c) 45%, transparent); }
  .mid .grade { color: #1a1300; }
  .compact .grade { width: 24px; height: 24px; font-size: 14px; border-radius: 6px; }
  .compact .head { padding: 5px 8px; }
  .txt { flex: 1; display: flex; flex-direction: column; min-width: 0; }
  .verdict { font-weight: 600; font-size: 13px; }
  .score { font-weight: 400; color: var(--text-3); font-size: 12px; margin-left: 4px; font-variant-numeric: tabular-nums; }
  .stage { font-size: 11.5px; color: var(--text-3); }
  .why { display: flex; align-items: center; gap: 3px; font-size: 12px; color: var(--text-2); }
  .list { list-style: none; margin: 0; padding: 2px 10px 8px 10px; display: flex; flex-direction: column; gap: 5px; user-select: text; }
  .list li { display: grid; grid-template-columns: 18px 1fr auto; gap: 6px; align-items: start; font-size: 12.5px; line-height: 1.35; }
  .ic { width: 17px; height: 17px; border-radius: 50%; display: grid; place-items: center; margin-top: 1px; }
  .good .ic { background: color-mix(in srgb, var(--ok) 22%, transparent); color: var(--ok); }
  .info .ic { background: var(--surface-strong); color: var(--text-2); }
  .warn .ic { background: color-mix(in srgb, var(--warn) 22%, transparent); color: var(--warn); }
  .bad .ic { background: color-mix(in srgb, var(--danger) 22%, transparent); color: var(--danger); }
  .bad .ft { color: var(--text); font-weight: 600; }
  .pts { font-size: 11px; color: var(--text-3); font-variant-numeric: tabular-nums; }
  .again { margin: 0 0 8px 8px; }
</style>
