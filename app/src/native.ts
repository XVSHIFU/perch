import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {getCurrentWindow} from '@tauri-apps/api/window';
export const isNative = '__TAURI_INTERNALS__' in window;
export const desktop = {
  exportDiagnostics: () => invoke<string|null>('export_diagnostics'),
  window: (action:string) => invoke('window_action',{action}),
  isMaximized: () => getCurrentWindow().isMaximized(),
  onResized: (fn:()=>void) => getCurrentWindow().onResized(fn),
  startDemo: (id:string) => invoke<string>('start_demo',{id}),
  openDemo: (id:string) => invoke('open_demo',{id}),
  stopDemo: (id:string) => invoke('stop_demo',{id}),
  stopAll: () => invoke('stop_all'),
  external: (url:string) => invoke('open_reference',{url}),
  onClose: (fn:()=>void)=>listen('desktop-close-requested',fn),
  onRunning: (fn:()=>void)=>listen('desktop-running-requested',fn),
  onStopped: (fn:()=>void)=>listen('desktop-all-stopped',fn),
};
