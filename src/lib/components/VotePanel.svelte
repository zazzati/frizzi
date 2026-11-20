<script lang="ts">
 import { resolve } from '$app/paths';
 import { CalendarDays } from '@lucide/svelte';
 import { Button } from '#lib/components/ui/button/index.ts';
 import EventCalendar from './EventCalendar.svelte';
 import Feedback from './Feedback.svelte';
 import { formatDate, formatTime, lang, plural, t } from '#lib/i18n.ts';
 import type { EventPage, SelectionSource } from '#lib/event-page.ts';
 import type { EventData } from '#lib/types.ts';

 export let view: EventPage;
 export let event: EventData;
 const { busy, error, notice, nearVote, month, name, email, selected, results, zones } = view;
 // Which controls picked the dates, reported once the vote is saved.
 const sources = new Set<SelectionSource>();
 $: open = event.status==='OPEN' && !event.voting_ended;

 function selectSlot(id: string, source: SelectionSource) {
   const slot = event.slots.find(slot=>slot.id===id);
   if (!slot || $busy || event.status!=='OPEN' || event.voting_ended) return;
   sources.add(source);
   $month = slot.date.slice(0,7);
   if ($selected.includes(id)) $selected = $selected.filter(chosen=>chosen!==id);
   else $selected = event.allow_multiple_choices ? [...$selected,id] : [id];
 }
</script>

<EventCalendar slots={event.slots} selected={$selected} confirmedId={event.confirmed_slot_id}
  bind:month={$month} interactive={open} busy={$busy} choose={id=>selectSlot(id,'calendar')}
  mirrored={open} markedLabel="" counts={$results.counts} total={event.participants.length}/>
{#if open}
 <form on:submit|preventDefault={()=>view.submitVote(sources)}>
  <fieldset disabled={$busy}>
   <fieldset>
   <legend class="vote-legend">{event.allow_multiple_choices?$t.voteText:$t.one}</legend>
   <div class="date-list">{#each event.slots as slot}
    {@const count=$results.counts.get(slot.id)||0}
    {@const yours=$zones.viewerTime(slot)}
    <label class="date-option" class:chosen={$selected.includes(slot.id)}>
    <span class="date-item">
     <span class="date-item-icon"><CalendarDays size={16} aria-hidden="true"/></span>
     <span>
      <strong><time datetime={slot.date}>{formatDate(slot.date,$lang)}</time></strong>
      <small>{formatTime(slot.start,$lang)}{slot.end?' – '+formatTime(slot.end,$lang):''}</small>
      {#if yours}<small>{$t.yourTime}: {yours}</small>{/if}
      {#if event.participants.length}<small>{$plural('availableCount',count)}</small>{/if}
     </span>
    </span>
    <input type={event.allow_multiple_choices?'checkbox':'radio'} name="dates" aria-label={`${formatDate(slot.date,$lang)} ${formatTime(slot.start,$lang)}${yours?` (${$t.yourTime}: ${yours})`:''}${event.participants.length?' · '+$plural('availableCount',count):''}`} checked={$selected.includes(slot.id)} on:change={()=>selectSlot(slot.id,'list')}/>
   </label>{/each}</div>
   <p class="fine">{$t.noDates}</p>
   </fieldset>
   <label class="mt-5">{$t.name} *<input name="participant_name" required maxlength="100" autocomplete="name" bind:value={$name}/></label>
   <label class="mt-5">{$t.email} *<input name="participant_email" required type="email" maxlength="255" autocomplete="email" bind:value={$email}
    aria-invalid={$error==='email_exists'} aria-describedby={$error==='email_exists'?'feedback-error':undefined}/></label>
   <p class="fine">{$t.votePrivacy} <a href={resolve('/privacy')}>{$t.privacyTitle}</a></p>
   {#if $nearVote && ($notice || $error)}<div class="vote-feedback">
    <Feedback notice={$notice} error={$error} target={view.noticeTarget}/>
   </div>{/if}
   <Button type="submit" class="submit" disabled={$busy}>{view.mode==='edit'?$t.save:$t.send}</Button>
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
