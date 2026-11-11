import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { viteSingleFile } from 'vite-plugin-singlefile';

// One self contained dist/index.html: JS and CSS are inlined, so the page
// opens straight from disk (file://) with no server and no network.
export default defineConfig({
  base: './',
  plugins: [svelte(), viteSingleFile()],
  server: { fs: { allow: ['..'] } },
  build: { target: 'es2022', chunkSizeWarningLimit: 4000 },
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
});
