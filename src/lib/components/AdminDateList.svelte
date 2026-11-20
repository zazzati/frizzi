<script lang="ts">
 import { CalendarDays } from '@lucide/svelte';
 import { Button } from '#lib/components/ui/button/index.ts';
 import { formatDate, formatTime, lang, plural, t } from '#lib/i18n.ts';
 import type { EventPage } from '#lib/event-page.ts';
 import type { EventData } from '#lib/types.ts';

 export let view: EventPage;
 export let event: EventData;
 /** The date picked on the calendar, outlined in the list. */
 export let highlighted: string | null;
 export let confirm: (id: string) => void;
 const { busy, tiebreakSaving, results, zones } = view;
</script>

<div class="date-list">{#each event.slots as slot}
 {@const count=$results.counts.get(slot.id)||0}
 {@const leading=$results.leadingSlots.some(leader=>leader.id===slot.id)}
 {@const yours=$zones.viewerTime(slot)}
 <div class="date-option" id={`option-${slot.id}`} tabindex="-1" class:calendar-target={highlighted===slot.id} class:chosen={event.confirmed_slot_id===slot.id || leading} class:tiebreak-picked={$results.decisiveSlot?.id===slot.id}>
 <div class="date-item">
  <span class="date-item-icon"><CalendarDays size={16} aria-hidden="true"/></span>
  <span>
   <strong><time datetime={slot.date}>{formatDate(slot.date,$lang)}</time></strong>
   <small>{formatTime(slot.start,$lang)}{slot.end?' – '+formatTime(slot.end,$lang):''}</small>
   {#if yours}<small>{$t.yourTime}: {yours}</small>{/if}
   <small>{$plural('availableOf',count,{total:event.participants.length})}{leading?` · ${$t.leadingDate}`:''}</small>
  </span>
 </div>
 <Button id={`confirm-${slot.id}`} variant="outline" class="small-button" disabled={$busy || $tiebreakSaving || event.confirmed_slot_id===slot.id} onclick={()=>confirm(slot.id)} aria-label={`${$t.confirm} · ${formatDate(slot.date,$lang)}`}>{$t.confirm}</Button>
</div>{/each}</div>

<style>
 .date-option { padding: 12px 14px; }
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
 .date-option.tiebreak-picked .date-item-icon { background: #e6d6f7; color: #472963; }
</style>
