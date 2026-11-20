<script lang="ts">
 import { resolve } from '$app/paths';
 import { page } from '$app/state';
 import { RotateCw } from '@lucide/svelte';
 import { Button } from '#lib/components/ui/button/index.ts';
 import { t } from '#lib/i18n.ts';
 const missing = $derived(page.status===404);
</script>

<svelte:head><title>{missing?$t.pageNotFound:$t.errorTitle} · Frizzi</title></svelte:head>

<section class="center-card">
 <h1>{missing?$t.pageNotFound:$t.errorTitle}</h1>
 <p>{missing?$t.notFound:$t.serviceUnavailable}</p>
 <div class="error-actions">
  {#if !missing}<Button onclick={()=>location.reload()}><RotateCw size={16} aria-hidden="true"/>{$t.retry}</Button>{/if}
  <a href={resolve('')}>{$t.createEvent}</a>
 </div>
</section>
