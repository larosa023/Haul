import { invoke } from '@tauri-apps/api/core';
document.getElementById('app')!.textContent = 'Haul';
void invoke('ui_ready');
