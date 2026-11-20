<script lang="ts">
 import { afterNavigate } from '$app/navigation';
 import { page } from '$app/state';
 import AdminActions from '#lib/components/AdminActions.svelte';
 import AdminDateList from '#lib/components/AdminDateList.svelte';
 import AdminLinkBox from '#lib/components/AdminLinkBox.svelte';
 import ConfirmDialog from '#lib/components/ConfirmDialog.svelte';
 import EditDetailsPanel from '#lib/components/EditDetailsPanel.svelte';
 import EventCalendar from '#lib/components/EventCalendar.svelte';
 import EventShell from '#lib/components/EventShell.svelte';
 import ParticipantMessage from '#lib/components/ParticipantMessage.svelte';
 import ShareActions from '#lib/components/ShareActions.svelte';
 import { EventPage } from '#lib/event.svelte.ts';
 import { t } from '#lib/i18n.ts';
 import type { Question } from '#lib/types.ts';

 const view = new EventPage('admin');
 let dialog: ConfirmDialog, statusActions: AdminActions;
 // The date the organizer picked on the calendar, until it is gone from the event.
 let adminSlot = $state<string | null>(null);
 const highlighted = $derived(view.event?.slots.some(slot=>slot.id===adminSlot) ? adminSlot : null);
 const ask = (question: Question) => dialog.ask(question);
 afterNavigate(() => {
   adminSlot = null;
   void view.open(page.params.token!, page.params.secret!);
 });

 function highlight(id: string) {
   adminSlot = id;
   const action = document.getElementById(`confirm-${id}`) as HTMLButtonElement | null;
   if (action && !action.disabled) action.focus();
   else document.getElementById(`option-${id}`)?.focus();
 }
</script>

<EventShell {view}>
 {#snippet actions(event)}<EditDetailsPanel {view} {event} {ask}/>{/snippet}
 {#snippet children(event)}
  <EventCalendar slots={event.slots} selected={highlighted?[highlighted]:[]} confirmedId={event.confirmed_slot_id}
    bind:month={view.month} interactive busy={view.busy} choose={highlight}
    mirrored markedLabel={$t.highlightedDate} counts={view.counts} total={event.participants.length}/>
  <AdminDateList {view} {event} {highlighted} confirm={id=>statusActions.setStatus('CONFIRMED',id)}/>
  <AdminActions bind:this={statusActions} {view} {event} {ask} saved={()=>adminSlot=null}/>
  <h3>{$t.shareTitle}</h3>
  <ShareActions url={view.publicUrl()} title={event.title} feedback={view.feedback}>
   {#snippet children(copy, copied)}
    <AdminLinkBox link={view.adminUrl()} {copied} copy={()=>copy(view.adminUrl())}/>
   {/snippet}
  </ShareActions>
 {/snippet}
 {#snippet messages(event)}
  <ParticipantMessage recipients={event.participants.length} {ask} send={view.sendMessage}/>
 {/snippet}
</EventShell>

<ConfirmDialog bind:this={dialog}/>
