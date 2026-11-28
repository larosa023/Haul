<script lang="ts">
  // A live speed graph: the last 90 seconds, newest on the right, scaled to the peak.
  // Its look follows the download tracking style (Settings → Appearance) unless given one.
  import { app } from '../state.svelte';

  let {
    data,
    up = [],
    height = 64,
    slots = 90,
    dot = true,
    variant,
  }: { data: number[]; up?: number[]; height?: number; slots?: number; dot?: boolean; variant?: string } = $props();
  const W = 300;
  const uid = `g${Math.random().toString(36).slice(2, 8)}`;

  const style = $derived(variant ?? app.settings?.progressStyle ?? 'glow');
  const peak = $derived(Math.max(1, ...data, ...up));
  const top = $derived(peak * 1.18);

  function pts(arr: number[]): [number, number][] {
    const off = slots - arr.length;
    return arr.map((v, i) => [((off + i) / (slots - 1)) * W, height - (v / top) * (height - 4) - 2]);
  }

  // Smooth line through the points (midpoint quadratic curves).
  function smooth(p: [number, number][]) {
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

  // Straight segments (classic, minimal) or steps (striped).
  function straight(p: [number, number][]) {
    return p.map(([x, y], i) => `${i ? 'L' : 'M'}${x.toFixed(1)},${y.toFixed(1)}`).join(' ');
  }
  function steps(p: [number, number][]) {
    if (!p.length) return '';
    let d = `M${p[0][0].toFixed(1)},${p[0][1].toFixed(1)}`;
    for (let i = 1; i < p.length; i++) d += ` H${p[i][0].toFixed(1)} V${p[i][1].toFixed(1)}`;
    return d;
  }

  const down = $derived(pts(data));
  const upPts = $derived(pts(up));
  const dLine = $derived(style === 'striped' ? steps(down) : style === 'classic' || style === 'minimal' ? straight(down) : smooth(down));
  const dArea = $derived(down.length > 1 ? `${dLine} L${W},${height} L${down[0][0].toFixed(1)},${height} Z` : '');
  const uLine = $derived(up.some((v) => v > 0) ? (style === 'striped' ? steps(upPts) : smooth(upPts)) : '');
  const last = $derived(down[down.length - 1]);
  // Blocks: one bar per second, heights snapped to 8 levels.
  const bw = $derived((W / slots) * 0.72);
  const bars = $derived(
    data.map((v, i) => {
      const level = Math.ceil((v / top) * 8) / 8;
      const h = Math.max(v > 0 ? 2 : 0, level * (height - 2));
      return { x: ((slots - data.length + i) / slots) * W, h };
    }),
  );
  const grid = $derived(style === 'minimal' ? [] : style === 'blocks' ? [0.25, 0.5, 0.75] : [0.25, 0.5, 0.75]);
</script>

<svg class="graph s-{style}" viewBox="0 0 {W} {height}" preserveAspectRatio="none" style="height: {height}px" aria-hidden="true">
  <defs>
    <linearGradient id="{uid}-fill" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="var(--accent)" stop-opacity={style === 'classic' ? 0.16 : 0.45} />
      <stop offset="100%" stop-color="var(--accent)" stop-opacity={style === 'classic' ? 0.16 : 0} />
    </linearGradient>
    <linearGradient id="{uid}-aurora" x1="0" y1="0" x2="1" y2="0" gradientUnits="objectBoundingBox" spreadMethod="repeat">
      <stop offset="0%" stop-color="var(--accent)" />
      <stop offset="35%" stop-color="color-mix(in oklch, var(--accent), #ff4fd8 60%)" />
      <stop offset="70%" stop-color="color-mix(in oklch, var(--accent), #3ce0ff 60%)" />
      <stop offset="100%" stop-color="var(--accent)" />
      <animateTransform attributeName="gradientTransform" type="translate" values="-1 0; 1 0" dur="6s" repeatCount="indefinite" />
    </linearGradient>
    <linearGradient id="{uid}-aurora-fade" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="white" stop-opacity="0.5" />
      <stop offset="100%" stop-color="white" stop-opacity="0" />
    </linearGradient>
    <mask id="{uid}-fade"><rect x="0" y="0" width={W} height={height} fill="url(#{uid}-aurora-fade)" /></mask>
    <pattern id="{uid}-stripes" width="8" height="8" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
      <rect width="8" height="8" fill="color-mix(in srgb, var(--accent) 14%, transparent)" />
      <rect width="4" height="8" fill="color-mix(in srgb, var(--accent) 38%, transparent)" />
      <animateTransform attributeName="patternTransform" type="translate" additive="sum" values="0 0; 8 0" dur="0.8s" repeatCount="indefinite" />
    </pattern>
  </defs>
  {#each grid as g (g)}<line x1="0" x2={W} y1={height * g} y2={height * g} class="grid" />{/each}

  {#if style === 'blocks'}
    {#each bars as b, i (i)}<rect class="bar" class:now={i === bars.length - 1} x={b.x} y={height - b.h} width={bw} height={b.h} />{/each}
  {:else if style === 'aurora'}
    {#if dArea}<path d={dArea} fill="url(#{uid}-aurora)" mask="url(#{uid}-fade)" />{/if}
    {#if uLine}<path d={uLine} class="up" />{/if}
    {#if dLine}<path d={dLine} class="down" stroke="url(#{uid}-aurora)" />{/if}
  {:else}
    {#if dArea && style !== 'minimal' && style !== 'neon'}<path d={dArea} fill={style === 'striped' ? `url(#${uid}-stripes)` : `url(#${uid}-fill)`} />{/if}
    {#if uLine}<path d={uLine} class="up" />{/if}
    {#if dLine}<path d={dLine} class="down" />{/if}
  {/if}
</svg>
{#if last && dot && style !== 'blocks' && style !== 'minimal'}
  <!-- The "now" dot sits outside the stretched SVG so it stays round. -->
  <span class="now s-{style}" style="right: 0; top: {last[1]}px"></span>
{/if}

<style>
  .graph { display: block; width: 100%; overflow: visible; }
  .grid { stroke: var(--line); stroke-width: 1; vector-effect: non-scaling-stroke; }
  .down { fill: none; stroke: var(--accent); stroke-width: 2; vector-effect: non-scaling-stroke; stroke-linejoin: round; filter: drop-shadow(0 0 4px color-mix(in srgb, var(--accent) 60%, transparent)); }
  .up { fill: none; stroke: var(--ok); stroke-width: 1.5; stroke-dasharray: 3 3; vector-effect: non-scaling-stroke; opacity: 0.8; }
  .s-classic .down { stroke-width: 1.5; filter: none; }
  .s-striped .down { stroke-width: 1.5; filter: none; }
  .s-aurora .down { stroke-width: 2.5; filter: drop-shadow(0 0 6px color-mix(in srgb, var(--accent) 50%, transparent)); }
  .s-minimal .down { stroke-width: 1; filter: none; opacity: 0.9; }
  .s-minimal .up { opacity: 0.5; }
  .s-neon .grid { stroke: color-mix(in srgb, var(--accent) 18%, transparent); stroke-dasharray: 2 4; }
  .s-neon .down { stroke: color-mix(in srgb, var(--accent) 65%, #fff); stroke-width: 2; filter: drop-shadow(0 0 3px var(--accent)) drop-shadow(0 0 9px var(--accent)); }
  .bar { fill: color-mix(in srgb, var(--accent) 65%, transparent); }
  .bar.now { fill: var(--accent); }
  .now { position: absolute; width: 8px; height: 8px; margin: -4px -4px 0 0; border-radius: 50%; background: var(--accent); box-shadow: 0 0 0 0 var(--accent); animation: ping 1.6s ease-out infinite; transition: top 900ms linear; }
  .now.s-classic { animation: none; width: 6px; height: 6px; margin: -3px -3px 0 0; }
  .now.s-neon { background: #fff; box-shadow: 0 0 6px 2px var(--accent); animation: none; }
  @keyframes ping { 0% { box-shadow: 0 0 0 0 color-mix(in srgb, var(--accent) 70%, transparent); } 100% { box-shadow: 0 0 0 9px transparent; } }
  @media (prefers-reduced-motion: reduce) { .now { animation: none; } }
</style>
