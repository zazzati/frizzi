import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import tailwindcss from '@tailwindcss/vite';
import vercel from '@sveltejs/adapter-vercel';
import node from '@sveltejs/adapter-node';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

const base = (process.env.FRIZZI_BASE_PATH || '') as '' | `/${string}`;
const proxy = {
 [`${base}/api`]: {
  target: process.env.API_PROXY_TARGET || 'http://127.0.0.1:3001',
  rewrite: (path: string) => path.slice(base.length)
 }
};

export default defineConfig({
 plugins: [tailwindcss(), sveltekit({
  adapter: process.env.FRIZZI_SELF_HOST === '1'
   ? node({ out: process.env.FRIZZI_BUILD_DIR || 'build' })
   : vercel({ runtime: 'nodejs22.x' }),
  paths: { base },
  preprocess: vitePreprocess(),
  // Runes mode for the app's own components; libraries keep the mode they were written in.
  compilerOptions: { runes: ({ filename }) => filename.split(/[/\\]/).includes('node_modules') ? undefined : true }
 })],
 server: { proxy }
});
