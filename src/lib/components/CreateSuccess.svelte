<script lang="ts">
 import { onMount } from 'svelte';
 import { resolve } from '$app/paths';
 import { Check } from '@lucide/svelte';
 import ShareActions from './ShareActions.svelte';
 import { errorText, t } from '#lib/i18n.ts';
 /** `link` is the public link of the event just created. */
 let { link, title }: { link: string; title: string } = $props();
 let heading: HTMLElement;
 let errorCode = $state('');
 onMount(() => heading.focus());
</script>

<div class="party-scene party-scene--success">
 <div class="party-guests party-guests--toast" aria-hidden="true"></div>
 <section class="center-card party-card">
 <Check size={40} aria-hidden="true"/><h1 tabindex="-1" bind:this={heading}>{$t.success}</h1><p>{$t.checkEmail}</p>
 <div class="copy-row">
  <label class="grow">{$t.share}<input dir="ltr" readonly value={link}/></label>
  <ShareActions inline url={link} {title} feedback={code=>errorCode=code}/>
 </div>
 {#if errorCode}<p class="error" role="alert">{errorText(errorCode,$t)}</p>{/if}
 <a href={resolve('')}>{$t.back}</a>
 </section>
</div>
