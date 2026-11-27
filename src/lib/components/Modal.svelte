<script lang="ts">
  import type { Snippet } from 'svelte';
  import { onMount } from 'svelte';

  let { title, width = 560, onclose, children, footer }: { title: string; width?: number; onclose: () => void; children: Snippet; footer?: Snippet } = $props();
  let box: HTMLDivElement | undefined = $state();

  onMount(() => {
    const prev = document.activeElement as HTMLElement | null;
    const first = box?.querySelector<HTMLElement>('[autofocus], input:not([disabled]), textarea, select, button.primary:not([disabled])');
    // Nothing to focus yet (still loading): focus the dialog so Esc still closes it.
    (first ?? box)?.focus();
    return () => prev?.focus?.();
  });

  function key(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      onclose();
    }
    // Keep Tab inside the dialog.
    if (e.key === 'Tab' && box) {
      const f = [...box.querySelectorAll<HTMLElement>('button, input, select, textarea, [tabindex]:not([tabindex="-1"])')].filter((x) => !x.hasAttribute('disabled'));
      if (!f.length) return;
      const i = f.indexOf(document.activeElement as HTMLElement);
      if (e.shiftKey && i <= 0) {
        e.preventDefault();
        f[f.length - 1].focus();
      } else if (!e.shiftKey && i === f.length - 1) {
        e.preventDefault();
        f[0].focus();
      }
    }
  }
</script>

<div class="scrim" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="modal" role="dialog" aria-modal="true" aria-label={title} bind:this={box} style="width: min({width}px, calc(100vw - 40px))" onkeydown={key} tabindex="-1">
    <h2>{title}</h2>
    <div class="content">{@render children()}</div>
    {#if footer}<div class="footer">{@render footer()}</div>{/if}
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 50; display: grid; place-items: center; background: rgb(0 0 0 / 0.42); animation: fade 120ms ease; }
  .modal {
    max-height: calc(100vh - 60px); display: flex; flex-direction: column; border-radius: 10px; outline: none;
    background: var(--solid-bg, #1d1d22); border: 1px solid var(--line); box-shadow: 0 24px 60px rgb(0 0 0 / 0.45);
    animation: pop 160ms var(--ease);
  }
  h2 { margin: 0; padding: 20px 22px 6px; font: 600 18px var(--font-display); }
  .content { padding: 10px 22px 18px; overflow: auto; min-height: 0; }
  .footer { display: flex; justify-content: flex-end; align-items: center; gap: 8px; padding: 14px 22px; border-top: 1px solid var(--line); background: color-mix(in srgb, var(--surface) 60%, transparent); border-radius: 0 0 10px 10px; }
  @keyframes fade { from { opacity: 0; } }
  @keyframes pop { from { opacity: 0; transform: scale(0.97) translateY(6px); } }
</style>
