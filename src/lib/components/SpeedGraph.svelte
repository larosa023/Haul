<script lang="ts">
  // A live speed graph: the last 90 seconds, newest on the right, scaled to the peak.
  let { data, up = [], height = 64, slots = 90, dot = true }: { data: number[]; up?: number[]; height?: number; slots?: number; dot?: boolean } = $props();
  const W = 300;
  const uid = `g${Math.random().toString(36).slice(2, 8)}`;

  const peak = $derived(Math.max(1, ...data, ...up));
  const top = $derived(peak * 1.18);

  function pts(arr: number[]): [number, number][] {
    const off = slots - arr.length;
    return arr.map((v, i) => [((off + i) / (slots - 1)) * W, height - (v / top) * (height - 4) - 2]);
  }

  // Smooth line through the points (midpoint quadratic curves).
  function line(p: [number, number][]) {
    if (!p.length) return '';
    let d = `M${p[0][0].toFixed(1)},${p[0][1].toFixed(1)}`;
    for (let i = 1; i < p.length; i++) {
      const [x0, y0] = p[i - 1];
      const [x1, y1] = p[i];
      d += ` Q${x0.toFixed(1)},${y0.toFixed(1)} ${((x0 + x1) / 2).toFixed(1)},${((y0 + y1) / 2).toFixed(1)}`;
    }
    const [lx, ly] = p[p.length - 1];
    return d + ` L${lx.toFixed(1)},${ly.toFixed(1)}`;
  }

  const down = $derived(pts(data));
  const upPts = $derived(pts(up));
  const dLine = $derived(line(down));
  const dArea = $derived(down.length > 1 ? `${dLine} L${W},${height} L${down[0][0].toFixed(1)},${height} Z` : '');
  const uLine = $derived(up.some((v) => v > 0) ? line(upPts) : '');
  const last = $derived(down[down.length - 1]);
</script>

<svg class="graph" viewBox="0 0 {W} {height}" preserveAspectRatio="none" style="height: {height}px" aria-hidden="true">
  <defs>
    <linearGradient id="{uid}-fill" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="var(--accent)" stop-opacity="0.45" />
      <stop offset="100%" stop-color="var(--accent)" stop-opacity="0" />
    </linearGradient>
  </defs>
  {#each [0.25, 0.5, 0.75] as g (g)}<line x1="0" x2={W} y1={height * g} y2={height * g} class="grid" />{/each}
  {#if dArea}<path d={dArea} fill="url(#{uid}-fill)" />{/if}
  {#if uLine}<path d={uLine} class="up" />{/if}
  {#if dLine}<path d={dLine} class="down" />{/if}
</svg>
{#if last && dot}
  <!-- The "now" dot sits outside the stretched SVG so it stays round. -->
  <span class="now" style="right: 0; top: {last[1]}px"></span>
{/if}

<style>
  .graph { display: block; width: 100%; overflow: visible; }
  .grid { stroke: var(--line); stroke-width: 1; vector-effect: non-scaling-stroke; }
  .down { fill: none; stroke: var(--accent); stroke-width: 2; vector-effect: non-scaling-stroke; stroke-linejoin: round; filter: drop-shadow(0 0 4px color-mix(in srgb, var(--accent) 60%, transparent)); }
  .up { fill: none; stroke: var(--ok); stroke-width: 1.5; stroke-dasharray: 3 3; vector-effect: non-scaling-stroke; opacity: 0.8; }
  .now { position: absolute; width: 8px; height: 8px; margin: -4px -4px 0 0; border-radius: 50%; background: var(--accent); box-shadow: 0 0 0 0 var(--accent); animation: ping 1.6s ease-out infinite; transition: top 900ms linear; }
  @keyframes ping { 0% { box-shadow: 0 0 0 0 color-mix(in srgb, var(--accent) 70%, transparent); } 100% { box-shadow: 0 0 0 9px transparent; } }
</style>
