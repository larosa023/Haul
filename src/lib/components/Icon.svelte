<script lang="ts" module>
  // 24x24 stroke icons (Lucide-style), inlined so there's no icon library to ship.
  const PATHS: Record<string, string> = {
    all: 'M4 6h16M4 12h16M4 18h16',
    download: 'M12 4v12M7 11l5 5 5-5M4 20h16',
    upload: 'M12 20V8M7 13l5-5 5 5M4 4h16',
    seed: 'M12 20V10M8 14l4-4 4 4M5 4h14',
    check: 'M5 12.5l4.5 4.5L19 7.5',
    done: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM8 12.5l3 3 5-6',
    pause: 'M7 5h3.5v14H7zM13.5 5H17v14h-3.5z',
    play: 'M7 4.5v15l12-7.5z',
    alert: 'M12 9v4M12 17h.01M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z',
    plus: 'M12 5v14M5 12h14',
    x: 'M6 6l12 12M18 6 6 18',
    search: 'M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4-4',
    settings:
      'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z',
    trash: 'M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13',
    folder: 'M3 6a1 1 0 0 1 1-1h5l2 2h9a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z',
    file: 'M14 3H6a1 1 0 0 0-1 1v16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V8zM14 3v5h5',
    open: 'M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5',
    link: 'M10 14a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-1 1M14 10a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l1-1',
    magnet: 'M6 3v8a6 6 0 0 0 12 0V3h-4v8a2 2 0 0 1-4 0V3zM6 7h4M14 7h4',
    refresh: 'M20 11a8 8 0 0 0-14.9-4M4 4v4h4M4 13a8 8 0 0 0 14.9 4M20 20v-4h-4',
    up: 'M6 15l6-6 6 6',
    down: 'M6 9l6 6 6-6',
    top: 'M6 18l6-6 6 6M6 6h12',
    bottom: 'M6 6l6 6 6-6M6 18h12',
    chevronRight: 'M9 6l6 6-6 6',
    chevronLeft: 'M15 6l-6 6 6 6',
    gauge: 'M12 14l4-6M4.9 18a9 9 0 1 1 14.2 0',
    turtle: 'M5 15c0-4 3-7 7-7s7 3 7 7zM3 15h18M7 15v3M17 15v3M19 11l2-1',
    clock: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 7v5l3 2',
    list: 'M8 6h13M8 12h13M8 18h13M3.5 6h.01M3.5 12h.01M3.5 18h.01',
    info: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v5M12 8h.01',
    filter: 'M4 5h16l-6 7.5V19l-4 2v-8.5z',
    brush: 'M9.1 11.9l8-8.1a2.9 2.9 0 1 1 4.1 4.1l-8.1 8M7.1 14.9c-1.7 0-3 1.4-3 3 0 1.3-2.5 1.5-2 2 1.1 1.1 2.5 2 4 2 2.2 0 4-1.8 4-4a3 3 0 0 0-3-3z',
    globe: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18',
    shield: 'M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z',
    puzzle:
      'M10 3h4v3a2 2 0 1 0 4 0V3h0a2 2 0 0 1 2 2v4h-3a2 2 0 1 0 0 4h3v4a2 2 0 0 1-2 2h-4v-3a2 2 0 1 0-4 0v3H6a2 2 0 0 1-2-2v-4h3a2 2 0 1 0 0-4H4V5a2 2 0 0 1 2-2z',
    sort: 'M3 7h11M3 12h7M3 17h4M17 4v16M14 17l3 3 3-3',
    archive: 'M3 4h18v4H3zM5 8v12h14V8M10 12h4',
    network: 'M5 12a10 10 0 0 1 14 0M8.5 15.5a5 5 0 0 1 7 0M12 19h.01M2 8.5a15 15 0 0 1 20 0',
    copy: 'M9 9h11v11H9zM5 15H4V4h11v1',
    dots: 'M5.5 12h.01M12 12h.01M18.5 12h.01',
    keyboard: 'M3 6h18v12H3zM7 10h.01M11 10h.01M15 10h.01M7 14h10',
    cloud: 'M7 18a4.5 4.5 0 1 1 .9-8.9A6 6 0 0 1 19.5 11a3.5 3.5 0 0 1-.5 7z',
    key: 'M15 7a4 4 0 1 1-3.9 4.9L4 19v2H2v-3l7.1-7.1A4 4 0 0 1 15 7zM16 10h.01',
    sparkle: 'M12 3l2 6 6 2-6 2-2 6-2-6-6-2 6-2z',
  };
</script>

<script lang="ts">
  let { name, size = 18, stroke = 1.8, fill = false }: { name: string; size?: number; stroke?: number; fill?: boolean } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 24 24" fill={fill ? 'currentColor' : 'none'} stroke="currentColor" stroke-width={stroke}
  stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={PATHS[name] ?? ''} /></svg>
