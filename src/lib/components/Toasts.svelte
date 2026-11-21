<script lang="ts">
  import { ui, dismissToast } from '../state.svelte';
</script>

<div class="toasts" aria-live="polite">
  {#each ui.toasts as t (t.id)}
    <div class="toast {t.kind}">
      <span>{t.text}</span>
      {#if t.action}
        <button class="btn sm primary" onclick={() => { const a = t.action!; dismissToast(t.id); a.run(); }}>{t.action.label}</button>
      {/if}
      <button class="x" aria-label="Dismiss" onclick={() => dismissToast(t.id)}>×</button>
    </div>
  {/each}
</div>

<style>
  .toasts { position: fixed; right: 20px; bottom: 20px; z-index: 100; display: flex; flex-direction: column; gap: 8px; max-width: 440px; }
  .toast {
    display: flex; align-items: center; gap: 10px; padding: 10px 10px 10px 14px; border-radius: 8px; font-size: 13px;
    background: var(--solid-bg, #1d1d22); border: 1px solid var(--line); border-left: 3px solid var(--info); box-shadow: 0 10px 30px rgb(0 0 0 / 0.35);
    animation: in 180ms var(--ease);
  }
  .toast span { flex: 1; overflow-wrap: anywhere; }
  .success { border-left-color: var(--ok); }
  .error { border-left-color: var(--danger); }
  .x { border: 0; background: none; color: var(--text-3); font-size: 18px; line-height: 1; cursor: pointer; padding: 0 4px; }
  .x:hover { color: var(--text); }
  @keyframes in { from { opacity: 0; transform: translateY(8px); } }
</style>
