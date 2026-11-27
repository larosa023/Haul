import { mount } from 'svelte';
import './app.css';
import './lib/progress.css';

// The mini window loads the same page; give it only the small component.
const mini = typeof window !== 'undefined' && (window as unknown as { __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } } }).__TAURI_INTERNALS__?.metadata?.currentWindow?.label === 'mini';
const target = document.getElementById('app')!;

export default mini
  ? import('./Mini.svelte').then((m) => mount(m.default, { target }))
  : import('./App.svelte').then((m) => mount(m.default, { target }));
