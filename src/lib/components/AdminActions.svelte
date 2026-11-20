<script lang="ts">
 import { Button } from '#lib/components/ui/button/index.ts';
 import { formatDate, lang, t } from '#lib/i18n.ts';
 import type { Status } from '#lib/api.ts';
 import type { EventPage } from '#lib/event-page.ts';
 import type { EventData, Question } from '#lib/types.ts';

 export let view: EventPage;
 export let event: EventData;
 /** The page's confirmation dialog. */
 export let ask: (question: Question) => Promise<boolean>;
 /** Called once the API accepted the change, before the event reloads. */
 export let saved: () => void = () => {};
 const { busy, tiebreakSaving } = view;

 /** Confirms a date, reopens or cancels the event; the date list confirms through this too. */
 export async function setStatus(status: Status, id: string | null = null) {
   if ($tiebreakSaving) return;
   // Every status change emails all participants, so it always needs an explicit confirmation.
   const slot = event.slots.find(slot=>slot.id===id);
   const question = status==='CONFIRMED'?{title:$t.confirmDateTitle,text:$t.confirmDateText,detail:slot&&formatDate(slot.date,$lang),confirm:$t.confirmDateAction,cancel:$t.goBack}
     :status==='OPEN'?{title:$t.reopenTitle,text:$t.reopenText,confirm:$t.reopen,cancel:$t.goBack}
     :{title:$t.cancelPrompt,text:$t.cancelText,confirm:$t.cancel,cancel:$t.keepEvent,danger:true};
   if (!await ask(question)) return;
   await view.setStatus(status, id, saved);
 }
</script>

<div class="admin-actions">
 <Button variant="ghost" disabled={$busy || $tiebreakSaving || event.status==='OPEN'} onclick={()=>setStatus('OPEN')}>{$t.reopen}</Button>
 <Button variant="ghost" class="danger" disabled={$busy || $tiebreakSaving || event.status==='CANCELLED'} onclick={()=>setStatus('CANCELLED')}>{$t.cancel}</Button>
</div>
