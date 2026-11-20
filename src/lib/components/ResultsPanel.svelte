<script lang="ts">
 import type { Snippet } from 'svelte';
 import { PartyPopper, Star } from '@lucide/svelte';
 import { formatDate, intlLocale, lang, plural, t } from '#lib/i18n.ts';
 import { numberFormat } from '#lib/intl.ts';
 import type { EventPage } from '#lib/event.svelte.ts';
 import type { EventData } from '#lib/types.ts';

 type Props = {
   view: EventPage;
   event: EventData;
   /** Shown under the heading, such as the organizer's message to participants. */
   children?: Snippet;
 };
 let { view, event, children }: Props = $props();
 // Only the organizer breaks ties.
 const admin = $derived(view.mode==='admin');
 const numbers = $derived(numberFormat(intlLocale($lang)));
</script>

<section class="panel responses">
 <h2>{$t.people} <span class="count">{numbers.format(event.participants.length)}</span></h2>
 {@render children?.()}
 {#if !event.participants.length}<p>{admin?$t.noPeopleOrganizer:$t.noPeople}</p>{:else}
  {#if view.leadingSlots.length}
   <section class="result-leaders" aria-labelledby="result-leaders-heading">
    <div class="leaders-title">
     <PartyPopper size={21} aria-hidden="true"/>
     <h3 id="result-leaders-heading">{view.decisiveSlot?$t.tiebreakWinner:view.leadingSlots.length===1?$t.topDate:$t.tiedDates}</h3>
     <span class="leaders-count">{$plural('availableOf',view.maxVotes,{total:event.participants.length})}</span>
    </div>
    <ul>
     {#each view.leadingSlots as slot (slot.id)}
      <li>
       <div class="leader-date">
        <time datetime={slot.date}>{formatDate(slot.date,$lang)}</time>
        {#if view.decisiveSlot}<span class="tiebreak-badge"><Star size={14} aria-hidden="true"/>{$t.tiebreakBadge}</span>{/if}
        {#if admin && view.tiedLeaders.length>1 && !view.decisiveSlot}
         <button type="button" class="tiebreak-action" disabled={view.busy||view.tiebreakSaving} onclick={()=>view.setTiebreak(slot.id)}
          aria-label={`${$t.tiebreakVote} · ${formatDate(slot.date,$lang)}`}><Star size={14} aria-hidden="true"/>{$t.tiebreakVote}</button>
        {/if}
       </div>
      </li>
     {/each}
    </ul>
    {#if admin && view.decisiveSlot}
     <button type="button" class="tiebreak-action tiebreak-undo" disabled={view.busy||view.tiebreakSaving} onclick={()=>view.setTiebreak(null)}>{$t.tiebreakUndo}</button>
    {/if}
   </section>
  {/if}
  <ul class="participants" role="list" aria-label={$t.allParticipants}>{#each event.participants as person}<li><bdi>{person.name}</bdi></li>{/each}</ul>
  <div class="results">{#each event.slots as slot}
   {@const count=view.counts.get(slot.id)||0}
   <div class="result"><strong>{formatDate(slot.date,$lang)}</strong>
    {#if event.status==='CONFIRMED' && view.decisiveSlot?.id===slot.id}<span class="tiebreak-badge"><Star size={14} aria-hidden="true"/>{$t.tiebreakBadge}</span>{/if}
    <span>{$plural('availableCount',count)}</span><div class="bar"><i style={`width:${100*count/event.participants.length}%`}></i></div><small>{#each event.participants.filter(p=>p.slots.includes(slot.id)) as person,i}{i?', ':''}<bdi>{person.name}</bdi>{/each}</small>
   </div>
  {/each}</div>
 {/if}
</section>
