<script lang="ts">
 import { PartyPopper, Star } from '@lucide/svelte';
 import { formatDate, intlLocale, lang, plural, t } from '#lib/i18n.ts';
 import { numberFormat } from '#lib/intl.ts';
 import type { EventPage } from '#lib/event-page.ts';
 import type { EventData } from '#lib/types.ts';

 export let view: EventPage;
 export let event: EventData;
 const { busy, tiebreakSaving, results } = view;
 // Only the organizer breaks ties.
 $: admin = view.mode==='admin';
 $: numbers = numberFormat(intlLocale($lang));
 $: ({ decisiveSlot, leadingSlots, tiedLeaders, maxVotes, counts } = $results);
</script>

<section class="panel responses">
 <h2>{$t.people} <span class="count">{numbers.format(event.participants.length)}</span></h2>
 <slot/>
 {#if !event.participants.length}<p>{admin?$t.noPeopleOrganizer:$t.noPeople}</p>{:else}
  {#if leadingSlots.length}
   <section class="result-leaders" aria-labelledby="result-leaders-heading">
    <div class="leaders-title">
     <PartyPopper size={21} aria-hidden="true"/>
     <h3 id="result-leaders-heading">{decisiveSlot?$t.tiebreakWinner:leadingSlots.length===1?$t.topDate:$t.tiedDates}</h3>
     <span class="leaders-count">{$plural('availableOf',maxVotes,{total:event.participants.length})}</span>
    </div>
    <ul>
     {#each leadingSlots as slot (slot.id)}
      <li>
       <div class="leader-date">
        <time datetime={slot.date}>{formatDate(slot.date,$lang)}</time>
        {#if decisiveSlot}<span class="tiebreak-badge"><Star size={14} aria-hidden="true"/>{$t.tiebreakBadge}</span>{/if}
        {#if admin && tiedLeaders.length>1 && !decisiveSlot}
         <button type="button" class="tiebreak-action" disabled={$busy||$tiebreakSaving} on:click={()=>view.setTiebreak(slot.id)}
          aria-label={`${$t.tiebreakVote} · ${formatDate(slot.date,$lang)}`}><Star size={14} aria-hidden="true"/>{$t.tiebreakVote}</button>
        {/if}
       </div>
      </li>
     {/each}
    </ul>
    {#if admin && decisiveSlot}
     <button type="button" class="tiebreak-action tiebreak-undo" disabled={$busy||$tiebreakSaving} on:click={()=>view.setTiebreak(null)}>{$t.tiebreakUndo}</button>
    {/if}
   </section>
  {/if}
  <ul class="participants" role="list" aria-label={$t.allParticipants}>{#each event.participants as person}<li><bdi>{person.name}</bdi></li>{/each}</ul>
  <div class="results">{#each event.slots as slot}
   {@const count=counts.get(slot.id)||0}
   <div class="result"><strong>{formatDate(slot.date,$lang)}</strong>
    {#if event.status==='CONFIRMED' && decisiveSlot?.id===slot.id}<span class="tiebreak-badge"><Star size={14} aria-hidden="true"/>{$t.tiebreakBadge}</span>{/if}
    <span>{$plural('availableCount',count)}</span><div class="bar"><i style={`width:${100*count/event.participants.length}%`}></i></div><small>{#each event.participants.filter(p=>p.slots.includes(slot.id)) as person,i}{i?', ':''}<bdi>{person.name}</bdi>{/each}</small>
   </div>
  {/each}</div>
 {/if}
</section>
