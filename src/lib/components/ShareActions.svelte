<script lang="ts">
 import { onMount, type Snippet } from 'svelte';
 import { Copy, Share2 } from '@lucide/svelte';
 import { Button } from '#lib/components/ui/button/index.ts';
 import { t } from '#lib/i18n.ts';
 import { track } from '#lib/telemetry.ts';

 type Props = {
   /** The public event link. */
   url: string;
   title: string;
   /** Buttons only, for a row that already holds the link field; otherwise the buttons get their own row. */
   inline?: boolean;
   /** Called with '' when a copy starts and with `copy_failed` when the clipboard refuses it. */
   feedback: (code: string) => void;
   /** Further links to copy, such as the organizer's, share the confirmation and its announcement. */
   children?: Snippet<[copy: (text: string) => void, copied: boolean]>;
 };
 let { url, title, inline = false, feedback, children }: Props = $props();

 let copied = $state<'' | 'public' | 'other'>('');
 let canShare = $state(false);
 let copiedTimer: ReturnType<typeof setTimeout> | undefined;
 onMount(() => {
   canShare = typeof navigator.share === 'function';
   return () => clearTimeout(copiedTimer);
 });

 async function copy(text = url, which: 'public' | 'other' = 'public') {
   feedback('');
   try {
     await navigator.clipboard.writeText(text);
     if (which === 'public') track('share_copy');
     copied = which;
     clearTimeout(copiedTimer); copiedTimer = setTimeout(() => copied = '', 2000);
   }
   catch { track('browser_clipboard'); feedback('copy_failed'); }
 }

 async function share() {
   try { await navigator.share({ title, url }); track('share_native'); }
   catch (error) { if (!(error instanceof DOMException && error.name === 'AbortError')) await copy(); }
 }
</script>

{#if inline}
 <Button onclick={()=>copy()}><Copy size={16}/>{copied?$t.copied:$t.copy}</Button><span class="sr-only" role="status">{copied?$t.copied:''}</span>
 {#if canShare}<Button variant="outline" onclick={share}><Share2 size={16}/>{$t.shareAction}</Button>{/if}
{:else}
 <div class="flex flex-wrap gap-2">
  <Button onclick={()=>copy()}><Copy size={16}/>{copied==='public'?$t.copied:$t.copy}</Button>
  {#if canShare}<Button variant="outline" onclick={share}><Share2 size={16}/>{$t.shareAction}</Button>{/if}
 </div>
 <span class="sr-only" role="status">{copied?$t.copied:''}</span>
 {@render children?.(text => copy(text, 'other'), copied==='other')}
{/if}
