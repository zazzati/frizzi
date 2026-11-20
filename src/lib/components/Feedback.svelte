<script lang="ts">
 import type { Action } from 'svelte/action';
 import { errorText, t } from '#lib/i18n.ts';
 import type { NoticeKey } from '#lib/event-page.ts';
 export let notice: NoticeKey | null;
 export let error: string;
 /** Lets the page focus the notice once it shows. */
 export let target: Action<HTMLElement>;
</script>

{#if notice}<p class="notice" tabindex="-1" use:target>{$t[notice]}</p>{/if}
{#if error}<p class="error" role="alert" id="feedback-error">{errorText(error,$t)}</p>{/if}
