<script lang="ts">
 import { ArrowUpRight, CalendarClock, CalendarDays, CircleCheck, CircleX, Clock, LockKeyhole, MapPin, Navigation, Users, Vote } from '@lucide/svelte';
 import LocationAttribution from './LocationAttribution.svelte';
 import { formatDate, formatTime, lang, t } from '#lib/i18n.ts';
 import type { PollState } from '#lib/poll.ts';
 import type { EventData, Slot } from '#lib/types.ts';

 type Props = {
   event: EventData;
   pollState: PollState;
   confirmed: Slot | undefined;
   /** Zone names in the page language. */
   eventZone: string;
   localZone: string;
   /** The viewer's own time for a slot, when it differs from the event's. */
   viewerTime: (slot: Slot) => string | null;
   calendarHref: string;
 };
 let { event, pollState, confirmed, eventZone, localZone, viewerTime, calendarHref }: Props = $props();

 const pollStates = {
   open: { icon: Vote, label: 'open' },
   closed: { icon: LockKeyhole, label: 'pollClosed' },
   confirmed: { icon: CircleCheck, label: 'confirmed' },
   cancelled: { icon: CircleX, label: 'cancelled' }
 } as const;

 function webLocation(text: string) {
   try {
     const url = new URL(text);
     if (url.protocol === 'https:' || url.protocol === 'http:') return url.href;
   } catch { /* Plain place names and addresses are handled separately. */ }
   return null;
 }
 const locationLink = $derived(!event.location_geo ? webLocation(event.location) : null);
 const StateIcon = $derived(pollStates[pollState].icon);
</script>

<section class="event-header">
 <span class="poll-status" data-state={pollState}>
  <StateIcon size={17} aria-hidden="true"/>
  {$t[pollStates[pollState].label]}
 </span>
 <h1><bdi>{event.title}</bdi></h1>
 {#if event.description?.trim()}
  <p class="event-description">{event.description}</p>
 {/if}
 <div class="event-details">
  <div class="event-place">
   <span class="place-pin"><MapPin size={19} aria-hidden="true"/></span>
   <div class="place-content">
    <strong><bdi>{event.location}</bdi></strong>
    {#if event.location_geo?.kind==='approximate'}
     <small>{$t.approximateLocation}</small>
    {/if}
    {#if event.location_geo}<LocationAttribution source={event.location_geo.attribution}/>{/if}
   </div>
   {#if event.location_geo}
    {@const coordinates=`${event.location_geo.latitude},${event.location_geo.longitude}`}
    <div class="place-actions">
     <a class="place-directions" href={`https://www.google.com/maps/dir/?api=1&destination=${encodeURIComponent(coordinates)}`} target="_blank" rel="noopener noreferrer"><Navigation size={15} aria-hidden="true"/>{$t.directions}</a>
     {#if event.location_geo.kind==='venue'}
      <a class="place-link" href={`https://www.google.com/maps/search/?api=1&query=${encodeURIComponent(event.location)}`} target="_blank" rel="noopener noreferrer" aria-label={$t.viewOnGoogleMaps}>Google Maps<ArrowUpRight size={15} class="rtl:-scale-x-100" aria-hidden="true"/></a>
     {/if}
    </div>
   {:else if locationLink}
    <div class="place-actions">
     <a class="place-directions" href={locationLink} target="_blank" rel="noopener noreferrer">{$t.openLocationLink}<ArrowUpRight size={15} class="rtl:-scale-x-100" aria-hidden="true"/></a>
    </div>
   {/if}
  </div>
  <div class="event-facts">
   <span class="event-fact"><Users size={16} aria-hidden="true"/><span>{$t.organized} <strong><bdi>{event.organizer_name}</bdi></strong></span></span>
   <span class="event-fact" title={event.timezone}><Clock size={16} aria-hidden="true"/><span class="sr-only">{$t.timezone}</span><bdi>{eventZone}</bdi></span>
  </div>
 </div>
 {#if confirmed}
  {@const yours=viewerTime(confirmed)}
  <div class="event-confirmed flex flex-wrap items-center justify-between gap-3">
   <div><strong>{$t.confirmedDate}: {formatDate(confirmed.date,$lang)}</strong><br/>{formatTime(confirmed.start,$lang)}{confirmed.end?' – '+formatTime(confirmed.end,$lang):''} · <span title={event.timezone}>{eventZone}</span>
    {#if yours}<small class="zone-hint">{$t.yourTime}: {yours} · {localZone}</small>{/if}</div>
   <a class="button btn-outline gap-2" href={calendarHref} download="frizzi.ics"><CalendarDays size={16}/>{$t.addCalendar}</a>
  </div>
 {:else if event.status==='OPEN' && event.voting_deadline}
  <div class="vote-deadline" class:expired={event.voting_ended}>
   <CalendarClock size={23} aria-hidden="true"/>
   <div>
    <span class="deadline-label">{$t.votingDeadline}</span>
    <strong><time datetime={event.voting_deadline}>{formatDate(event.voting_deadline,$lang)}</time></strong>
    <small>{$t.deadlineHint} (<span title={event.timezone}>{eventZone}</span>)</small>
   </div>
  </div>
 {/if}
</section>

<style>
 .poll-status {
   display: inline-flex;
   align-items: center;
   gap: 7px;
   padding: 5px 10px;
   border: 1px solid #c8dfbc;
   border-radius: 999px;
   background: #edf7e6;
   color: #254c32;
   font-size: 13px;
   font-weight: 700;
   line-height: 1.3;
 }
 .poll-status[data-state="closed"] { background: #fff1d2; color: #62430f; border-color: #e9ca88; }
 .poll-status[data-state="confirmed"] { background: #e9f1ff; color: #224a81; border-color: #bfd1ee; }
 .poll-status[data-state="cancelled"] { background: #fee9e9; color: #842c3a; border-color: #edbdc5; }
 .poll-status :global(svg) { flex-shrink: 0; }
 .event-header .event-description {
   max-width: 74ch;
   margin: 12px 0 16px;
   border-inline-start: 3px solid #9bc775;
   padding-inline-start: 12px;
   color: #3d5345;
   font: 400 16px/1.55 var(--font-ui);
   white-space: pre-wrap;
   overflow-wrap: anywhere;
 }
 .event-details { margin-top: 12px; }
 .event-place {
   display: flex;
   flex-wrap: wrap;
   align-items: center;
   gap: 8px 12px;
   padding: 10px 12px;
   border: 1px solid #dce8d3;
   border-radius: 12px;
   background: #eff6e9;
 }
 .place-pin {
   display: grid;
   place-items: center;
   flex: 0 0 36px;
   height: 36px;
   border-radius: 10px;
   background: #d4edb6;
   color: #2d593b;
 }
 .place-content {
   flex: 1 1 200px;
   min-width: min(100% - 48px, 130px);
   overflow-wrap: anywhere;
 }
 .place-content strong {
   display: block;
   font: 700 16px/1.35 var(--font-ui);
   color: #203b2b;
 }
 .place-content small { display: block; color: #4f634e; font-size: 12px; }
 .place-content :global(p) { margin-top: 2px; line-height: 1.35; }
 .place-actions { display: flex; align-items: center; flex-wrap: wrap; gap: 4px 12px; }
 .place-actions a {
   display: inline-flex;
   align-items: center;
   gap: 6px;
   min-height: 34px;
   font-size: 13px;
   font-weight: 700;
 }
 .place-actions a:focus-visible { outline: 2px solid #47762e; outline-offset: 3px; }
 .place-directions { padding: 4px 10px; border-radius: 8px; background: #254b35; color: white; text-decoration: none; }
 .place-directions:hover { background: #356245; }
 .place-link { color: #32593e; text-underline-offset: 3px; }
 .event-facts { display: flex; flex-wrap: wrap; gap: 6px 8px; margin-top: 8px; }
 .event-fact {
   display: inline-flex;
   align-items: center;
   gap: 6px;
   max-width: 100%;
   padding: 4px 9px;
   border: 1px solid #e5e8de;
   border-radius: 8px;
   background: #fff;
   color: #4b6050;
   font-size: 13px;
   overflow-wrap: anywhere;
 }
 .event-fact :global(svg) { flex-shrink: 0; color: #55734d; }
 .event-fact strong { color: #283f31; }
</style>
