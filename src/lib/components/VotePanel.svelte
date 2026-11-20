<script lang="ts">
 import { resolve } from '$app/paths';
 import { CalendarDays } from '@lucide/svelte';
 import { Button } from '#lib/components/ui/button/index.ts';
 import EventCalendar from './EventCalendar.svelte';
 import Feedback from './Feedback.svelte';
 import { formatDate, formatTime, lang, plural, t } from '#lib/i18n.ts';
 import type { EventPage, SelectionSource } from '#lib/event.svelte.ts';
 import type { EventData } from '#lib/types.ts';
 import { onSubmit } from '#lib/utils.ts';

 let { view, event }: { view: EventPage; event: EventData } = $props();
 // Which controls picked the dates, reported once the vote is saved.
 const sources = new Set<SelectionSource>();
 const open = $derived(event.status==='OPEN' && !event.voting_ended);

 function selectSlot(id: string, source: SelectionSource) {
   const slot = event.slots.find(slot=>slot.id===id);
   if (!slot || view.busy || event.status!=='OPEN' || event.voting_ended) return;
   sources.add(source);
   view.month = slot.date.slice(0,7);
   if (view.selected.includes(id)) view.selected = view.selected.filter(chosen=>chosen!==id);
   else view.selected = event.allow_multiple_choices ? [...view.selected,id] : [id];
 }
</script>

<EventCalendar slots={event.slots} selected={view.selected} confirmedId={event.confirmed_slot_id}
  bind:month={view.month} interactive={open} busy={view.busy} choose={id=>selectSlot(id,'calendar')}
  mirrored={open} markedLabel="" counts={view.counts} total={event.participants.length}/>
{#if open}
 <form onsubmit={onSubmit(()=>view.submitVote(sources))}>
  <fieldset disabled={view.busy}>
   <fieldset>
   <legend class="vote-legend">{event.allow_multiple_choices?$t.voteText:$t.one}</legend>
   <div class="date-list">{#each event.slots as slot}
    {@const count=view.counts.get(slot.id)||0}
    {@const yours=view.viewerTime(slot)}
    <label class="date-option" class:chosen={view.selected.includes(slot.id)}>
    <span class="date-item">
     <span class="date-item-icon"><CalendarDays size={16} aria-hidden="true"/></span>
     <span>
      <strong><time datetime={slot.date}>{formatDate(slot.date,$lang)}</time></strong>
      <small>{formatTime(slot.start,$lang)}{slot.end?' – '+formatTime(slot.end,$lang):''}</small>
      {#if yours}<small>{$t.yourTime}: {yours}</small>{/if}
      {#if event.participants.length}<small>{$plural('availableCount',count)}</small>{/if}
     </span>
    </span>
    <input type={event.allow_multiple_choices?'checkbox':'radio'} name="dates" aria-label={`${formatDate(slot.date,$lang)} ${formatTime(slot.start,$lang)}${yours?` (${$t.yourTime}: ${yours})`:''}${event.participants.length?' · '+$plural('availableCount',count):''}`} checked={view.selected.includes(slot.id)} onchange={()=>selectSlot(slot.id,'list')}/>
   </label>{/each}</div>
   <p class="fine">{$t.noDates}</p>
   </fieldset>
   <label class="mt-5">{$t.name} *<input name="participant_name" required maxlength="100" autocomplete="name" bind:value={view.name}/></label>
   <label class="mt-5">{$t.email} *<input name="participant_email" required type="email" maxlength="255" autocomplete="email" bind:value={view.email}
    aria-invalid={view.error==='email_exists'} aria-describedby={view.error==='email_exists'?'feedback-error':undefined}/></label>
   <p class="fine">{$t.votePrivacy} <a href={resolve('/privacy')}>{$t.privacyTitle}</a></p>
   {#if view.nearVote && (view.notice || view.error)}<div class="vote-feedback">
    <Feedback notice={view.notice} error={view.error} target={view.noticeTarget}/>
   </div>{/if}
   <Button type="submit" class="submit" disabled={view.busy}>{view.mode==='edit'?$t.save:$t.send}</Button>
  </fieldset>
 </form>
{:else}<p>{event.status==='OPEN'?$t.deadlinePassed:event.status==='CONFIRMED'?$t.confirmed:$t.cancelled}</p>{/if}

<style>
 .date-option { padding: 12px 14px; }
 label.date-option { flex-wrap: nowrap; }
 label.date-option input { flex-shrink: 0; }
 .date-item { display: flex; align-items: center; gap: 9px; min-width: 0; overflow-wrap: anywhere; }
 .date-item-icon {
   display: grid;
   place-items: center;
   flex: 0 0 31px;
   height: 31px;
   border-radius: 8px;
   background: #eef4ea;
   color: #40664b;
 }
 .date-option.chosen .date-item-icon { background: #dcefd1; color: #315838; }
</style>
