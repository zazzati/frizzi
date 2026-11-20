<script lang="ts">
 import type { Attachment } from 'svelte/attachments';
 import { errorText, t } from '#lib/i18n.ts';
 import type { NoticeKey } from '#lib/event.svelte.ts';
 type Props = {
   notice: NoticeKey | null;
   error: string;
   /** Lets the page focus the notice once it shows. */
   target: Attachment<HTMLElement>;
 };
 let { notice, error, target }: Props = $props();
</script>

{#if notice}<p class="notice" tabindex="-1" {@attach target}>{$t[notice]}</p>{/if}
{#if error}<p class="error" role="alert" id="feedback-error">{errorText(error,$t)}</p>{/if}
