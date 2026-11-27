<script lang="ts">
  // Each connection's part of the file, filling as it downloads (like IDM's map).
  let { segments, size }: { segments: { start: number; end: number; done: number }[]; size: number } = $props();
  const parts = $derived(
    size > 0
      ? segments.map((s) => {
          const len = Math.max(1, s.end - s.start);
          return { left: (s.start / size) * 100, width: (len / size) * 100, fill: Math.min(100, (s.done / len) * 100), active: s.done < len };
        })
      : [],
  );
</script>

<div class="map" role="img" aria-label="Download parts">
  {#each parts as p, i (i)}
    <span class="part" class:active={p.active} style="left: {p.left}%; width: {p.width}%">
      <span class="fill" style="width: {p.fill}%"></span>
    </span>
  {/each}
</div>

<style>
  .map { position: relative; height: 14px; border-radius: 4px; background: var(--surface-strong); overflow: hidden; }
  .part { position: absolute; top: 0; bottom: 0; box-shadow: inset -1px 0 0 var(--bg); }
  .fill { position: absolute; inset: 0 auto 0 0; background: color-mix(in srgb, var(--accent) 75%, var(--surface-strong)); transition: width 900ms linear; }
  .active .fill { background: linear-gradient(90deg, color-mix(in srgb, var(--accent) 70%, var(--surface-strong)), var(--accent)); }
  .active .fill::after { content: ''; position: absolute; right: 0; top: 0; bottom: 0; width: 2px; background: #fff; opacity: 0.55; box-shadow: 0 0 6px 1px var(--accent); }
  .part:not(.active) .fill { background: color-mix(in srgb, var(--accent) 55%, var(--surface-strong)); }
</style>
