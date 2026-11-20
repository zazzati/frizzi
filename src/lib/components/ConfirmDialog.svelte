<script lang="ts">
 import { tick } from 'svelte';
 import { Button } from '#lib/components/ui/button/index.ts';
 import type { Question } from '#lib/types.ts';
 let dialog: HTMLDialogElement;
 let question: Question = { title: '', text: '', confirm: '', cancel: '' };
 let answer: ((confirmed: boolean) => void) | undefined;
 let trigger: HTMLElement | null = null;

 /** Opens the modal and resolves true only when the explicit confirmation button is used. */
 export async function ask(next: Question) {
   answer?.(false);
   trigger = document.activeElement instanceof HTMLElement ? document.activeElement : null;
   question = next;
   await tick();
   dialog.returnValue = '';
   dialog.showModal();
   return new Promise<boolean>(resolve => answer = resolve);
 }

 function closed() {
   answer?.(dialog.returnValue === 'confirm');
   answer = undefined;
   if (trigger?.isConnected) trigger.focus();
 }
</script>

<dialog class="confirm-dialog" bind:this={dialog} aria-labelledby="confirm-dialog-title" aria-describedby="confirm-dialog-text" on:close={closed}>
 <form method="dialog">
  <h2 id="confirm-dialog-title">{question.title}</h2>
  <p id="confirm-dialog-text">{#if question.detail}<strong>{question.detail}</strong><br/>{/if}{question.text}</p>
  <div class="confirm-actions">
   <Button type="submit" variant="outline" value="cancel">{question.cancel}</Button>
   <Button type="submit" class={question.danger ? 'danger-button' : ''} value="confirm">{question.confirm}</Button>
  </div>
 </form>
</dialog>

<style>
 .confirm-dialog { width: min(100% - 32px, 460px); margin: auto; padding: 24px; border: 1px solid #e2e9df; border-radius: 16px; color: #182923; box-shadow: 0 18px 50px #1f3d1b33; }
 .confirm-dialog::backdrop { background: #18292366; }
 h2 { font-size: 22px; letter-spacing: normal; margin: 0 0 8px; }
 p { margin: 0; color: #3d5345; font-size: 15px; }
 .confirm-actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 8px; margin-top: 22px; }
 .confirm-actions :global(.danger-button) { background: #a3372f; }
 .confirm-actions :global(.danger-button:hover) { background: #8c2c25; }
 @media (max-width: 480px) { .confirm-actions { flex-direction: column-reverse; } .confirm-actions :global(.button) { width: 100%; } }
</style>
