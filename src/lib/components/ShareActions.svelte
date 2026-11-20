<script lang="ts">
 import { onDestroy, onMount } from 'svelte';
 import { Copy, Share2 } from '@lucide/svelte';
 import { Button } from '#lib/components/ui/button/index.ts';
 import { t } from '#lib/i18n.ts';
 import { track } from '#lib/telemetry.ts';

 /** The public event link. */
 export let url: string;
 export let title: string;
 /** Buttons only, for a row that already holds the link field; otherwise the buttons get their own row. */
 export let inline = false;
 /** Called with '' when a copy starts and with `copy_failed` when the clipboard refuses it. */
 export let feedback: (code: string) => void;

 let copied: '' | 'public' | 'other' = '';
 let canShare = false;
 let copiedTimer: ReturnType<typeof setTimeout> | undefined;
 onMount(() => canShare = typeof navigator.share === 'function');
 onDestroy(() => clearTimeout(copiedTimer));

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
 <!-- Further links to copy, such as the organizer's, share the confirmation and its announcement. -->
 <slot copy={(text: string) => copy(text, 'other')} copied={copied==='other'}/>
{/if}
