<script lang="ts">
 import { resolve } from '$app/paths';
 import { RotateCw } from '@lucide/svelte';
 import { Button } from '#lib/components/ui/button/index.ts';
 import EventHeader from './EventHeader.svelte';
 import Feedback from './Feedback.svelte';
 import ResultsPanel from './ResultsPanel.svelte';
 import { errorText, t } from '#lib/i18n.ts';
 import type { EventPage } from '#lib/event-page.ts';
 import availabilityParty from '#lib/assets/availability-party.webp';

 /**
  * The frame of the guest, participant and organizer pages: loader, error, header, page feedback
  * and the two columns. The default slot fills the first column; `actions` goes above the columns
  * and `messages` at the top of the responses.
  */
 export let view: EventPage;
 const { event, loading, refreshing, error, notice, nearVote, results, zones } = view;
</script>

{#if !$event && $loading}
 <div class="center-card" role="status">{$t.loading}</div>
{:else if $event}
 {@const data=$event}
 <div class="event-layout" aria-busy={$refreshing}>
  <EventHeader event={data} pollState={$results.pollState} confirmed={$results.confirmed} zones={$zones} calendarHref={view.calendarHref()}/>
  {#if !$nearVote}<div class="feedback-area">
   <Feedback notice={$notice} error={$error} target={view.noticeTarget}/>
  </div>{/if}
  <slot name="actions" event={data}/>
  <div class="party-scene mt-5">
   {#if data.status !== 'CANCELLED'}
    <div class="availability-party">
     <img src={availabilityParty} alt="" width="552" height="184" loading="lazy" decoding="async"/>
    </div>
   {/if}
   <div class="event-columns party-card">
   <section class="panel">
    <h2>{view.mode==='admin'?$t.admin:view.mode==='edit'?$t.edit:$t.vote}</h2>
    <slot event={data}/>
   </section>
   <ResultsPanel {view} event={data}><slot name="messages" event={data}/></ResultsPanel>
   </div>
  </div>
 </div>
{:else}
 <section class="center-card"><h1>{$error==='not_found'?$t.pageNotFound:$t.errorTitle}</h1><p class="error" role="alert">{errorText($error||'not_found',$t)}</p>
  <div class="error-actions">
   {#if $error.startsWith('service_unavailable')||$error==='rate_limited'}<Button onclick={view.retry}><RotateCw size={16} aria-hidden="true"/>{$t.retry}</Button>{/if}
   <a href={resolve('')}>{$t.createEvent}</a>
  </div>
 </section>
{/if}
