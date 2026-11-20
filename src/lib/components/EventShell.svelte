<script lang="ts">
 import type { Snippet } from 'svelte';
 import { resolve } from '$app/paths';
 import { RotateCw } from '@lucide/svelte';
 import { Button } from '#lib/components/ui/button/index.ts';
 import EventHeader from './EventHeader.svelte';
 import Feedback from './Feedback.svelte';
 import ResultsPanel from './ResultsPanel.svelte';
 import { errorText, t } from '#lib/i18n.ts';
 import type { EventPage } from '#lib/event.svelte.ts';
 import type { EventData } from '#lib/types.ts';
 import availabilityParty from '#lib/assets/availability-party.webp';

 /**
  * The frame of the guest, participant and organizer pages: loader, error, header, page feedback
  * and the two columns. `children` fills the first column; `actions` goes above the columns and
  * `messages` at the top of the responses.
  */
 type Props = {
   view: EventPage;
   children: Snippet<[EventData]>;
   actions?: Snippet<[EventData]>;
   messages?: Snippet<[EventData]>;
 };
 let { view, children, actions, messages }: Props = $props();
</script>

{#if !view.event && view.loading}
 <div class="center-card" role="status">{$t.loading}</div>
{:else if view.event}
 {@const event=view.event}
 <div class="event-layout" aria-busy={view.refreshing}>
  <EventHeader {event} pollState={view.pollState} confirmed={view.confirmed} eventZone={view.eventZone} localZone={view.localZoneName}
   viewerTime={view.viewerTime} calendarHref={view.calendarHref()}/>
  {#if !view.nearVote}<div class="feedback-area">
   <Feedback notice={view.notice} error={view.error} target={view.noticeTarget}/>
  </div>{/if}
  {@render actions?.(event)}
  <div class="party-scene mt-5">
   {#if event.status !== 'CANCELLED'}
    <div class="availability-party">
     <img src={availabilityParty} alt="" width="552" height="184" loading="lazy" decoding="async"/>
    </div>
   {/if}
   <div class="event-columns party-card">
   <section class="panel">
    <h2>{view.mode==='admin'?$t.admin:view.mode==='edit'?$t.edit:$t.vote}</h2>
    {@render children(event)}
   </section>
   <ResultsPanel {view} {event}>{@render messages?.(event)}</ResultsPanel>
   </div>
  </div>
 </div>
{:else}
 <section class="center-card"><h1>{view.error==='not_found'?$t.pageNotFound:$t.errorTitle}</h1><p class="error" role="alert">{errorText(view.error||'not_found',$t)}</p>
  <div class="error-actions">
   {#if view.error.startsWith('service_unavailable')||view.error==='rate_limited'}<Button onclick={view.retry}><RotateCw size={16} aria-hidden="true"/>{$t.retry}</Button>{/if}
   <a href={resolve('')}>{$t.createEvent}</a>
  </div>
 </section>
{/if}
