import {defineConfig} from 'vite';

export default defineConfig({
  server: {
    // Tauri watches native sources itself; Rust build outputs can be locked on Windows.
    watch: {ignored: ['**/src-tauri/**']},
  },
});
