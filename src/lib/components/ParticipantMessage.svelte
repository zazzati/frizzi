<script lang="ts">
 import { Bold, Italic, Mail, Send, Underline } from '@lucide/svelte';
 import { Button } from '#lib/components/ui/button/index.ts';
 import { errorText, intlLocale, lang, plural, t } from '#lib/i18n.ts';
 import { messagePreview } from '#lib/message.ts';
 import { numberFormat } from '#lib/intl.ts';
 import type { Question } from '#lib/types.ts';

 export let recipients: number;
 export let send: (message: string, requestId: string) => Promise<{queued: number}>;
 /** The page's confirmation dialog: an email to every participant cannot be taken back. */
 export let ask: (question: Question) => Promise<boolean>;

 let field: HTMLTextAreaElement;
 let message = '';
 let sending = false;
 let queued: number | null = null;
 let error = '';
 let attempt: {message: string; id: string} | undefined;
 $: numbers = numberFormat(intlLocale($lang));
 // Same rules as the email: only the allowed tags render, everything else shows as typed.
 $: preview = messagePreview(message.trim());

 function format(tag: 'b' | 'i' | 'u') {
   const {selectionStart: start, selectionEnd: end} = field;
   const next = message.slice(0, start) + `<${tag}>` + message.slice(start, end) + `</${tag}>` + message.slice(end);
   if (next.length > 2000) return;
   message = next;
   field.value = next;
   field.focus();
   field.setSelectionRange(start + 3, end + 3);
 }

 async function submit() {
   if (sending || recipients === 0 || !preview.plain) return;
   const content = message.trim();
   if (!await ask({title: $t.messageConfirmTitle, text: $plural('messageConfirmText', recipients), confirm: $t.sendParticipantMessage, cancel: $t.goBack})) return;
   if (attempt?.message !== content) attempt = {message: content, id: crypto.randomUUID()};
   sending = true;
   error = '';
   queued = null;
   try {
     const result = await send(content, attempt.id);
     queued = result.queued;
     message = '';
     attempt = undefined;
   } catch (cause) {
     error = cause instanceof Error ? cause.message : 'error';
   } finally {
     sending = false;
   }
 }
</script>

<details class="participant-message">
 <summary><Mail size={17} aria-hidden="true"/>{$t.writeParticipants}</summary>
 {#if recipients === 0}
  <p class="fine">{$t.noPeopleOrganizer}</p>
 {:else}
  <form on:submit|preventDefault={submit}>
   <label for="participant-message">{$t.participantMessage}</label>
   <div class="formatting">
    <button type="button" aria-label={$t.messageBold} title={$t.messageBold} disabled={sending} on:click={()=>format('b')}><Bold size={16}/></button>
    <button type="button" aria-label={$t.messageItalic} title={$t.messageItalic} disabled={sending} on:click={()=>format('i')}><Italic size={16}/></button>
    <button type="button" aria-label={$t.messageUnderline} title={$t.messageUnderline} disabled={sending} on:click={()=>format('u')}><Underline size={16}/></button>
   </div>
   <textarea id="participant-message" rows="4" maxlength="2000" required disabled={sending}
    aria-describedby="participant-message-hint" bind:this={field} bind:value={message}></textarea>
   <p class="fine" id="participant-message-hint">{$t.messageFormatting}</p>
   {#if preview.plain}
    <section class="message-preview" aria-labelledby="message-preview-title">
     <h3 id="message-preview-title">{$t.messagePreview}</h3>
     <!-- messagePreview escapes everything except b, strong, i, em, u and br. -->
     <p>{@html preview.html}</p>
    </section>
   {/if}
   <div class="message-actions">
    <Button type="submit" disabled={sending||!preview.plain}><Send size={16} class="rtl:-scale-x-100" aria-hidden="true"/>{sending?$t.loading:$t.sendParticipantMessage}</Button>
    <span class="fine">{$t.messageRecipients} <strong>{numbers.format(recipients)}</strong></span>
   </div>
   {#if queued !== null}<p class="notice" role="status">{$plural('messageQueued',queued)}</p>{/if}
   {#if error}<p class="error" role="alert">{error==='no_participants'?$t.noPeopleOrganizer:errorText(error,$t)}</p>{/if}
  </form>
 {/if}
</details>

<style>
 .participant-message { margin: 12px 0 16px; padding: 10px 12px; border: 1px solid #dce8d3; border-radius: 10px; background: #f4f8ee; }
 summary { cursor: pointer; color: #263f2c; font-size: 14px; font-weight: 700; }
 summary :global(svg) { display: inline-block; vertical-align: middle; margin-inline-end: 5px; }
 form { margin-top: 12px; }
 .formatting { display: flex; gap: 6px; margin-top: 6px; }
 .formatting button { display: grid; place-items: center; padding: 5px; border: 1px solid var(--control-border); border-radius: 6px; color: #263f2c; background: white; cursor: pointer; }
 .message-preview { margin-top: 10px; padding: 10px 12px; border: 1px dashed var(--control-border); border-radius: 8px; background: white; }
 .message-preview h3 { margin: 0 0 4px; color: var(--muted); font-size: 13px; font-weight: 700; }
 .message-preview p { margin: 0; color: #203b2b; font-size: 15px; line-height: 1.5; overflow-wrap: anywhere; }
 .message-actions { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-top: 8px; }
</style>
