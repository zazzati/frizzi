<script lang="ts">
 import { resolve } from '$app/paths';
 import { page } from '$app/state';
 import { afterNavigate } from '$app/navigation';
 import { ArrowUpRight, Check, CircleCheck, CircleX, Copy, KeyRound, LockKeyhole, RotateCw, MapPin, Clock, Navigation, Users, Share2, CalendarDays, CalendarClock, PartyPopper, Star, Vote } from '@lucide/svelte';
 import { lang, t, plural, errorText, formatDate, formatTime, intlLocale } from '#lib/i18n.ts';
 import { track } from '#lib/telemetry.ts';
 import * as poll from '#lib/poll.ts';
 import { parsePath } from '#lib/route.ts';
 import { datesChanged, slotDraft, slotPayload, slotProblems } from '#lib/slots.ts';
 import { canonicalZone, convertTime, viewerZone, zoneName } from '#lib/timezones.ts';
 import { numberFormat } from '#lib/intl.ts';
 import { onDestroy, tick } from 'svelte';
 import LocationAttribution from '#lib/components/LocationAttribution.svelte';
 import EventCalendar from '#lib/components/EventCalendar.svelte';
 import EventFields from '#lib/components/EventFields.svelte';
 import ParticipantMessage from '#lib/components/ParticipantMessage.svelte';
 import ConfirmDialog from '#lib/components/ConfirmDialog.svelte';
 import { Button } from '#lib/components/ui/button/index.ts';
 import type { EventData, LocationGeo, SlotDraft } from '#lib/types.ts';
 import availabilityParty from '#lib/assets/availability-party.webp';

 let event:EventData|null=null;
 let mode=page.params.path?'loading':'create';
 let loading=false, refreshing=false, busy=false, tiebreakSaving=false, editingDetails=false, slotsChecked=false;
 let copied:''|'public'|'admin'='';
 // Errors and notices of the vote form show next to its button; everything else under the header.
 let feedbackScope:'page'|'vote'='page';
 let errorCode='', noticeKey:keyof typeof $t|null=null;
 let publicToken='', secret='', link='';
 let title='', description='', location='', organizer_name='', organizer_email='';
 let locationGeo:LocationGeo|null=null;
 let allow_multiple_choices=true, timezone='Europe/Rome', votingDeadline='';
 let canShare=false, recoveryOpen=false, recovering=false, recovered=false;
 let recoveryEmail='', recoveryError='';
 let slots:SlotDraft[]=[slotDraft()];
 let name='', email='', selected:string[]=[];
 const selectionSources=new Set<'calendar'|'list'>();
 let calendarMonth='', adminSlot:string|null=null;
 let generation=0, copiedTimer:ReturnType<typeof setTimeout>|undefined;
 let confirmDialog:ConfirmDialog, successHeading:HTMLElement, noticeElement:HTMLElement;
 onDestroy(()=>clearTimeout(copiedTimer));
 const pollStates = {
   open: { icon: Vote, label: 'open' },
   closed: { icon: LockKeyhole, label: 'pollClosed' },
   confirmed: { icon: CircleCheck, label: 'confirmed' },
   cancelled: { icon: CircleX, label: 'cancelled' }
 } as const;
 let pollState: keyof typeof pollStates='open';

 afterNavigate(()=>{ void initialize(); });
 async function initialize(){
   const current=++generation;
   calendarMonth=''; adminSlot=null;
   selectionSources.clear();
   tiebreakSaving=false;
   canShare=typeof navigator.share==='function';
   recoveryOpen=false; recovering=false; recovered=false; recoveryEmail=''; recoveryError='';
   errorCode=''; noticeKey=null; feedbackScope='page'; event=null; editingDetails=false; slotsChecked=false; name='';email='';selected=[];link='';copied='';
   title='';description='';location='';locationGeo=null;organizer_name='';organizer_email='';
   slots=[slotDraft()];votingDeadline='';allow_multiple_choices=true;
   const route=parsePath(page.params.path);
   if(!route){mode='event';errorCode='not_found';return;}
   ({mode,publicToken,secret}=route);
   if(mode==='create'){timezone=viewerZone();return;}
   await load(current);
 }
 async function request(url:string,method='GET',body?:unknown){
   const response=await fetch(resolve(url.slice(1)),{method,headers:body?{'Content-Type':'application/json'}:{},body:body?JSON.stringify(body):undefined,cache:'no-store'}).catch(()=>{
     track('browser_network');
     throw new Error('service_unavailable');
   });
   const reference=response.headers.get('x-request-id');
   const support=reference && /^[a-f0-9-]{36}$/.test(reference) ? `:${reference}` : '';
   if(response.status>=500){
     if(!reference) track('browser_invalid_response');
     throw new Error(`service_unavailable${support}`);
   }
   const data=await response.json().catch(()=>{
     track('browser_invalid_response');
     throw new Error(`service_unavailable${support}`);
   });
   if(!response.ok) throw new Error(typeof data?.error==='string'?data.error:'error');
   return data;
 }
 async function load(current=generation){
   // The full-page loader only covers the first load; refreshes keep the page mounted.
   loading=!event; refreshing=!!event;
   try{
     const path=mode==='admin'?`/admin/${secret}`:'';
     // A private participant link needs both the event and the saved answer: fetch them together.
     const [data,person]:[EventData,{name:string,email:string,slots:string[]}|undefined]=await Promise.all([
       request(`/api/events/${publicToken}${path}`),
       mode==='edit'?request(`/api/events/${publicToken}/participant/${secret}`):undefined
     ]);
     if(current!==generation)return;
     event=data;
     if(person){name=person.name;email=person.email;selected=person.slots;}
     if (!data.slots.some(slot=>slot.id===adminSlot)) adminSlot=null;
     // Pick the month on the first load (calendarMonth is empty) or when the shown month lost its dates.
     if (!data.slots.some(slot=>slot.date.startsWith(calendarMonth+'-'))) {
       const confirmedSlot=data.slots.find(slot=>slot.id===data.confirmed_slot_id);
       calendarMonth=(confirmedSlot || data.slots.find(slot=>selected.includes(slot.id)) || data.slots[0])?.date.slice(0,7) || '';
     }
   }catch(error){if(current===generation)errorCode=error instanceof Error?error.message:'error';}
   finally{if(current===generation){loading=false;refreshing=false;}}
 }
 function payload(){return {title,description,location,location_geo:locationGeo,organizer_name,locale:$lang||'it',allow_multiple_choices,timezone,voting_deadline:votingDeadline||null,slots:slotPayload(slots)};}
 async function validateSlots(creating=false){
   // From the first attempt on, each row shows what is wrong with it and clears as it is fixed.
   slotsChecked=true;
   const found=slotProblems(slots);
   if(found.size){
     const kinds=new Set(found.values());
     if(creating&&kinds.has('duplicateDate'))track('create_duplicate_date');
     if(creating&&kinds.has('endBeforeStart'))track('create_time_range');
     const index=slots.findIndex(slot=>found.has(slot.key));
     // Re-enable the form first: a disabled field cannot take focus.
     busy=false;await tick();
     (document.querySelector(found.get(slots[index].key)==='duplicateDate'?`#date-${index}`:`[name="end-${index}-hour"]`) as HTMLElement|null)?.focus();
     throw new Error('invalid_input');
   }
   if(!canonicalZone(timezone)){
     if(creating)track('create_timezone');
     throw new Error('invalid_input');
   }
 }
 async function create(){
   errorCode='';busy=true;
   try{
     await validateSlots(true);
     const data=await request('/api/events','POST',{...payload(),organizer_email});
     track(locationGeo?'location_suggested':'location_manual');
     link=new URL(resolve(data.public_token), window.location.href).href;
     mode='success';
     await tick();successHeading?.focus();
   }
   catch(error){errorCode=error instanceof Error?error.message:'error';}
   finally{busy=false;}
 }
 async function submitVote(){
   errorCode='';noticeKey=null;feedbackScope='vote';busy=true;
   try{
     await request(`/api/events/${publicToken}/${mode==='edit'?`participant/${secret}`:'votes'}`,mode==='edit'?'PUT':'POST',{name,email,locale:$lang||'it',slots:selected});
     if(selectionSources.size>1) track('vote_both');
     else if(selectionSources.has('calendar')) track('vote_calendar');
     else if(selectionSources.has('list')) track('vote_list');
     selectionSources.clear();
     if(mode!=='edit'){name='';email='';selected=[];}
     await load();
     await notice('saved');
   }catch(error){
     errorCode=error instanceof Error?error.message:'error';
     if(errorCode==='email_exists'){
       recoveryEmail=email;
       recoveryOpen=true;
     }
     if(errorCode==='deadline_passed' && event) event={...event,voting_ended:true};
   }
   finally{busy=false;}
 }
 // The notice takes focus: the control that triggered the save may be gone or disabled now.
 async function notice(key:NonNullable<typeof noticeKey>){
   noticeKey=key;await tick();noticeElement?.focus();
 }
 function selectSlot(id:string, source:'calendar'|'list') {
   const slot=event?.slots.find(slot=>slot.id===id);
   if (!slot || busy || event?.status!=='OPEN' || event.voting_ended) return;
   selectionSources.add(source);
   calendarMonth=slot.date.slice(0,7);
   if (selected.includes(id)) selected=selected.filter(chosen=>chosen!==id);
   else selected=event.allow_multiple_choices ? [...selected,id] : [id];
 }
 function focusAdminSlot(id:string) {
   adminSlot=id;
   const action=document.getElementById(`confirm-${id}`) as HTMLButtonElement | null;
   if (action && !action.disabled) action.focus();
   else document.getElementById(`option-${id}`)?.focus();
 }
 async function setStatus(status:'OPEN'|'CONFIRMED'|'CANCELLED',id:string|null=null){
   if(tiebreakSaving)return;
   // Every status change emails all participants, so it always needs an explicit confirmation.
   const slot=event?.slots.find(slot=>slot.id===id);
   const question=status==='CONFIRMED'?{title:$t.confirmDateTitle,text:$t.confirmDateText,detail:slot&&formatDate(slot.date,$lang),confirm:$t.confirmDateAction,cancel:$t.goBack}
     :status==='OPEN'?{title:$t.reopenTitle,text:$t.reopenText,confirm:$t.reopen,cancel:$t.goBack}
     :{title:$t.cancelPrompt,text:$t.cancelText,confirm:$t.cancel,cancel:$t.keepEvent,danger:true};
   if(!await confirmDialog.ask(question))return;
   errorCode='';noticeKey=null;feedbackScope='page';busy=true;
   try{await request(`/api/events/${publicToken}/admin/${secret}`,'PUT',{status,confirmed_slot_id:id,locale:$lang||'it'});adminSlot=null;await load();await notice('statusSaved');}
   catch(error){errorCode=error instanceof Error?error.message:'error';}
   finally{busy=false;}
 }
 async function setTiebreak(slotId:string|null){
   if(busy||tiebreakSaving)return;
   const current=generation;
   errorCode='';noticeKey=null;feedbackScope='page';tiebreakSaving=true;
   try{
     await request(`/api/events/${publicToken}/admin/${secret}/tiebreak`,'PUT',{slot_id:slotId});
     if(current===generation)await load(current);
   }catch(error){
     if(current!==generation)return;
     const code=error instanceof Error?error.message:'error';
     if(code==='tiebreak_unavailable'||code==='event_closed')await load(current);
     if(current===generation)errorCode=code;
   }
   finally{if(current===generation)tiebreakSaving=false;}
 }
 function editDetails(){
   if(!event)return;
   title=event.title;description=event.description||'';location=event.location;locationGeo=event.location_geo;organizer_name=event.organizer_name;
   allow_multiple_choices=event.allow_multiple_choices;timezone=event.timezone;votingDeadline=event.voting_deadline||'';slotsChecked=false;
   slots=event.slots.map(s=>slotDraft({...s,start:s.start.slice(0,5),end:s.end?.slice(0,5)||''}));
   editingDetails=!editingDetails;
 }
 async function saveDetails(){
   if(event && (timezone!==event.timezone || datesChanged(event.slots,slots))
     && !await confirmDialog.ask({title:$t.datesChangedTitle,text:$t.datesChangedHint,confirm:$t.save,cancel:$t.goBack}))return;
   errorCode='';noticeKey=null;feedbackScope='page';busy=true;
   try{await validateSlots();await request(`/api/events/${publicToken}/admin/${secret}/details`,'PUT',payload());editingDetails=false;await load();await notice('changesSaved');}
   catch(error){errorCode=error instanceof Error?error.message:'error';}
   finally{busy=false;}
 }
 function publicLink(){
   return link||new URL(resolve(publicToken), window.location.href).href;
 }
 function adminLink(){
   return new URL(resolve(`${publicToken}/admin/${secret}`), window.location.href).href;
 }
 async function share(){
   try {await navigator.share({title:event?.title||title,url:publicLink()});track('share_native');}
   catch(error){if(!(error instanceof DOMException && error.name==='AbortError')) await copy();}
 }
 async function recover(){
   const current=generation;
   recoveryError=''; recovered=false; recovering=true;
   try {
     await request(`/api/events/${publicToken}/recover`,'POST',{email:recoveryEmail});
     if(current===generation) recovered=true;
   } catch(error){
     if(current===generation) recoveryError=error instanceof Error?error.message:'error';
   } finally {
     if(current===generation) recovering=false;
   }
 }
 async function copy(which:'public'|'admin'='public'){
   errorCode='';feedbackScope='page';
   try{
     await navigator.clipboard.writeText(which==='admin'?adminLink():publicLink());
     if(which==='public')track('share_copy');
     copied=which;
     clearTimeout(copiedTimer);copiedTimer=setTimeout(()=>copied='',2000);
   }
   catch{track('browser_clipboard');errorCode='copy_failed';}
 }
 function retry(){errorCode='';void load();}
 /** The slot's time on the viewer's clock, when it differs from the time in the event's zone. */
 function viewerTime(slot:{date:string,start:string,end:string|null}){
   if(!event||!localZone)return null;
   const start=convertTime(slot.date,slot.start,event.timezone,localZone);
   if(!start||(start.date===slot.date&&start.time===slot.start.slice(0,5)))return null;
   const end=slot.end?convertTime(slot.date,slot.end,event.timezone,localZone):null;
   return `${start.date!==slot.date?formatDate(start.date,$lang)+', ':''}${formatTime(start.time,$lang)}${end?' – '+formatTime(end.time,$lang):''}`;
 }
 function webLocation(text:string){
   try {
     const url=new URL(text);
     if(url.protocol==='https:' || url.protocol==='http:') return url.href;
   }catch{ /* Plain place names and addresses are handled separately. */ }
   return null;
 }
 $: locationLink=event && !event.location_geo ? webLocation(event.location) : null;
 $: confirmed=poll.confirmedSlot(event);
 $: voteCounts=poll.countsFor(event);
 $: maxVotes=poll.maxVotes(voteCounts);
 $: tiedLeaders=poll.tiedLeaders(event,voteCounts);
 $: decisiveSlot=poll.decisiveSlot(event,tiedLeaders);
 $: leadingSlots=poll.leadingSlots(event,tiedLeaders,decisiveSlot);
 $: numbers=numberFormat(intlLocale($lang));
 $: pollState=poll.pollState(event);
 $: problems=slotsChecked?slotProblems(slots):new Map();
 $: voteFormShown=mode!=='admin'&&event?.status==='OPEN'&&!event.voting_ended;
 $: nearVote=feedbackScope==='vote'&&voteFormShown;
 // Read once in the browser: the server render has no viewer zone.
 let localZone='';
 $: if(event&&!localZone)localZone=viewerZone();
 $: eventZone=event?zoneName(event.timezone,intlLocale($lang)):'';
 $: localZoneName=localZone?zoneName(localZone,intlLocale($lang)):'';
</script>

{#if mode==='create'}
 <div class="party-scene party-scene--create">
  <div class="create-heading">
   <h1>{$t.create}</h1>
   <div class="party-guests party-guests--trio" aria-hidden="true"></div>
  </div>
  <form id="form" on:submit|preventDefault={create} class="form-shell party-card">
  <fieldset disabled={busy}>
   <EventFields {problems} bind:title bind:description bind:location bind:locationGeo bind:slots bind:allow_multiple_choices bind:timezone bind:votingDeadline/>
   <div class="divider"></div>
   <div class="section-heading"><div><h2>{$t.organizer}</h2><p>{$t.privacy} <a href={resolve('/privacy')}>{$t.privacyTitle}</a></p></div></div>
   <div class="form-grid">
    <label>{$t.name} *<input name="organizer_name" required maxlength="100" autocomplete="name" bind:value={organizer_name}/></label>
    <label>{$t.email} *<input name="organizer_email" type="email" required maxlength="255" autocomplete="email" bind:value={organizer_email}/></label>
   </div>
   {#if errorCode}<p class="error" role="alert">{errorText(errorCode,$t)}</p>{/if}
   <Button type="submit" class="submit" disabled={busy}>{busy?$t.creating:$t.submit}</Button>
  </fieldset>
  </form>
 </div>
{:else if mode==='success'}
 <div class="party-scene party-scene--success">
  <div class="party-guests party-guests--toast" aria-hidden="true"></div>
  <section class="center-card party-card">
  <Check size={40} aria-hidden="true"/><h1 tabindex="-1" bind:this={successHeading}>{$t.success}</h1><p>{$t.checkEmail}</p>
  <div class="copy-row">
   <label class="grow">{$t.share}<input dir="ltr" readonly value={link}/></label>
   <Button onclick={()=>copy()}><Copy size={16}/>{copied?$t.copied:$t.copy}</Button><span class="sr-only" role="status">{copied?$t.copied:''}</span>
   {#if canShare}<Button variant="outline" onclick={share}><Share2 size={16}/>{$t.shareAction}</Button>{/if}
  </div>
  {#if errorCode}<p class="error" role="alert">{errorText(errorCode,$t)}</p>{/if}
  <a href={resolve('')}>{$t.back}</a>
  </section>
 </div>
{:else if !event && (loading || mode==='loading')}
 <div class="center-card" role="status">{$t.loading}</div>
{:else if event}
 <div class="event-layout" aria-busy={refreshing}>
  <section class="event-header">
   <span class="poll-status" data-state={pollState}>
    <svelte:component this={pollStates[pollState].icon} size={17} aria-hidden="true"/>
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
      {#if yours}<small class="zone-hint">{$t.yourTime}: {yours} · {localZoneName}</small>{/if}</div>
     <a class="button btn-outline gap-2" href={resolve(`api/events/${publicToken}/calendar.ics`)} download="frizzi.ics"><CalendarDays size={16}/>{$t.addCalendar}</a>
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
  {#if !nearVote}<div class="feedback-area">
   {#if noticeKey}<p class="notice" tabindex="-1" bind:this={noticeElement}>{$t[noticeKey]}</p>{/if}
   {#if errorCode}<p class="error" role="alert" id="feedback-error">{errorText(errorCode,$t)}</p>{/if}
  </div>{/if}
  {#if mode==='admin'}
   <Button variant="outline" onclick={editDetails}>{editingDetails?$t.dismiss:$t.editDetails}</Button>
   {#if editingDetails}
    <section class="panel details-panel">
     <form on:submit|preventDefault={saveDetails}><fieldset disabled={busy}>
      <EventFields editing {problems} bind:title bind:description bind:location bind:locationGeo bind:slots bind:allow_multiple_choices bind:timezone bind:votingDeadline/>
      <label class="mt-5">{$t.organized}<input name="organizer_name" required maxlength="100" bind:value={organizer_name}/></label>
      <Button type="submit" class="submit" disabled={busy}>{$t.save}</Button>
     </fieldset></form>
    </section>
   {/if}
  {/if}
  <div class="party-scene mt-5">
   {#if event.status !== 'CANCELLED'}
    <div class="availability-party">
     <img src={availabilityParty} alt="" width="552" height="184" loading="lazy" decoding="async"/>
    </div>
   {/if}
   <div class="event-columns party-card">
   <section class="panel">
    <h2>{mode==='admin'?$t.admin:mode==='edit'?$t.edit:$t.vote}</h2>
    <EventCalendar slots={event.slots}
      selected={mode==='admin'?(adminSlot?[adminSlot]:[]):selected} confirmedId={event.confirmed_slot_id}
      bind:month={calendarMonth} interactive={mode==='admin'||(event.status==='OPEN'&&!event.voting_ended)}
      {busy} choose={mode==='admin'?focusAdminSlot:(id)=>selectSlot(id,'calendar')}
      mirrored={mode==='admin'||(event.status==='OPEN'&&!event.voting_ended)} markedLabel={mode==='admin'?$t.highlightedDate:''}
      counts={voteCounts} total={event.participants.length}/>
    {#if mode==='admin'}
     <div class="date-list">{#each event.slots as slot}
      {@const count=voteCounts.get(slot.id)||0}
      {@const leading=leadingSlots.some(leader=>leader.id===slot.id)}
      {@const yours=viewerTime(slot)}
      <div class="date-option" id={`option-${slot.id}`} tabindex="-1" class:calendar-target={adminSlot===slot.id} class:chosen={event.confirmed_slot_id===slot.id || leading} class:tiebreak-picked={decisiveSlot?.id===slot.id}>
      <div class="date-item">
       <span class="date-item-icon"><CalendarDays size={16} aria-hidden="true"/></span>
       <span>
        <strong><time datetime={slot.date}>{formatDate(slot.date,$lang)}</time></strong>
        <small>{formatTime(slot.start,$lang)}{slot.end?' – '+formatTime(slot.end,$lang):''}</small>
        {#if yours}<small>{$t.yourTime}: {yours}</small>{/if}
        <small>{$plural('availableOf',count,{total:event.participants.length})}{leading?` · ${$t.leadingDate}`:''}</small>
       </span>
      </div>
      <Button id={`confirm-${slot.id}`} variant="outline" class="small-button" disabled={busy || tiebreakSaving || event.confirmed_slot_id===slot.id} onclick={()=>setStatus('CONFIRMED',slot.id)} aria-label={`${$t.confirm} · ${formatDate(slot.date,$lang)}`}>{$t.confirm}</Button>
     </div>{/each}</div>
     <div class="admin-actions">
      <Button variant="ghost" disabled={busy || tiebreakSaving || event.status==='OPEN'} onclick={()=>setStatus('OPEN')}>{$t.reopen}</Button>
      <Button variant="ghost" class="danger" disabled={busy || tiebreakSaving || event.status==='CANCELLED'} onclick={()=>setStatus('CANCELLED')}>{$t.cancel}</Button>
     </div>
     <h3>{$t.shareTitle}</h3>
     <div class="flex flex-wrap gap-2">
      <Button onclick={()=>copy()}><Copy size={16}/>{copied==='public'?$t.copied:$t.copy}</Button>
      {#if canShare}<Button variant="outline" onclick={share}><Share2 size={16}/>{$t.shareAction}</Button>{/if}
     </div>
     <span class="sr-only" role="status">{copied?$t.copied:''}</span>
     <section class="admin-link" aria-labelledby="admin-link-title">
      <h3 id="admin-link-title"><KeyRound size={16} aria-hidden="true"/>{$t.adminLinkTitle}</h3>
      <p id="admin-link-note">{$t.adminLinkNote}</p>
      <div class="copy-row">
       <input class="grow" dir="ltr" readonly value={adminLink()} aria-labelledby="admin-link-title" aria-describedby="admin-link-note" on:focus={event=>event.currentTarget.select()}/>
       <Button variant="outline" onclick={()=>copy('admin')}><Copy size={16} aria-hidden="true"/>{copied==='admin'?$t.copied:$t.copy}</Button>
      </div>
     </section>
    {:else if event.status==='OPEN' && !event.voting_ended}
     <form on:submit|preventDefault={submitVote}>
      <fieldset disabled={busy}>
       <fieldset>
       <legend class="vote-legend">{event.allow_multiple_choices?$t.voteText:$t.one}</legend>
       <div class="date-list">{#each event.slots as slot}
        {@const count=voteCounts.get(slot.id)||0}
        {@const yours=viewerTime(slot)}
        <label class="date-option" class:chosen={selected.includes(slot.id)}>
        <span class="date-item">
         <span class="date-item-icon"><CalendarDays size={16} aria-hidden="true"/></span>
         <span>
          <strong><time datetime={slot.date}>{formatDate(slot.date,$lang)}</time></strong>
          <small>{formatTime(slot.start,$lang)}{slot.end?' – '+formatTime(slot.end,$lang):''}</small>
          {#if yours}<small>{$t.yourTime}: {yours}</small>{/if}
          {#if event.participants.length}<small>{$plural('availableCount',count)}</small>{/if}
         </span>
        </span>
        <input type={event.allow_multiple_choices?'checkbox':'radio'} name="dates" aria-label={`${formatDate(slot.date,$lang)} ${formatTime(slot.start,$lang)}${yours?` (${$t.yourTime}: ${yours})`:''}${event.participants.length?' · '+$plural('availableCount',count):''}`} checked={selected.includes(slot.id)} on:change={()=>selectSlot(slot.id,'list')}/>
       </label>{/each}</div>
       <p class="fine">{$t.noDates}</p>
       </fieldset>
       <label class="mt-5">{$t.name} *<input name="participant_name" required maxlength="100" autocomplete="name" bind:value={name}/></label>
       <label class="mt-5">{$t.email} *<input name="participant_email" required type="email" maxlength="255" autocomplete="email" bind:value={email}
        aria-invalid={errorCode==='email_exists'} aria-describedby={errorCode==='email_exists'?'feedback-error':undefined}/></label>
       <p class="fine">{$t.votePrivacy} <a href={resolve('/privacy')}>{$t.privacyTitle}</a></p>
       {#if nearVote && (noticeKey || errorCode)}<div class="vote-feedback">
        {#if noticeKey}<p class="notice" tabindex="-1" bind:this={noticeElement}>{$t[noticeKey]}</p>{/if}
        {#if errorCode}<p class="error" role="alert" id="feedback-error">{errorText(errorCode,$t)}</p>{/if}
       </div>{/if}
       <Button type="submit" class="submit" disabled={busy}>{mode==='edit'?$t.save:$t.send}</Button>
      </fieldset>
     </form>
    {:else}<p>{event.status==='OPEN'?$t.deadlinePassed:event.status==='CONFIRMED'?$t.confirmed:$t.cancelled}</p>{/if}
    {#if mode!=='admin'}
     <details class="mt-5" bind:open={recoveryOpen}>
      <summary class="cursor-pointer text-sm font-semibold">{$t.recoverLink}</summary>
      <form class="mt-2" on:submit|preventDefault={recover}>
       <label>{$t.email}<input name="recovery_email" type="email" required maxlength="255" autocomplete="email" bind:value={recoveryEmail}/></label>
       <Button type="submit" variant="outline" class="mt-2" disabled={recovering}>{$t.recoverSend}</Button>
       {#if recovered}<p class="notice mt-2" role="status">{$t.recoverAccepted}</p>{/if}
       {#if recoveryError}<p class="error mt-2" role="alert">{errorText(recoveryError,$t)}</p>{/if}
      </form>
     </details>
    {/if}
   </section>
   <section class="panel responses">
    <h2>{$t.people} <span class="count">{numbers.format(event.participants.length)}</span></h2>
    {#if mode==='admin'}
     <ParticipantMessage recipients={event.participants.length} ask={question=>confirmDialog.ask(question)}
      send={(message,request_id)=>request(`/api/events/${publicToken}/admin/${secret}/messages`,'POST',{message,request_id})}/>
    {/if}
    {#if !event.participants.length}<p>{mode==='admin'?$t.noPeopleOrganizer:$t.noPeople}</p>{:else}
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
           {#if mode==='admin' && tiedLeaders.length>1 && !decisiveSlot}
            <button type="button" class="tiebreak-action" disabled={busy||tiebreakSaving} on:click={()=>setTiebreak(slot.id)}
             aria-label={`${$t.tiebreakVote} · ${formatDate(slot.date,$lang)}`}><Star size={14} aria-hidden="true"/>{$t.tiebreakVote}</button>
           {/if}
          </div>
         </li>
        {/each}
       </ul>
       {#if mode==='admin' && decisiveSlot}
        <button type="button" class="tiebreak-action tiebreak-undo" disabled={busy||tiebreakSaving} on:click={()=>setTiebreak(null)}>{$t.tiebreakUndo}</button>
       {/if}
      </section>
     {/if}
     <ul class="participants" role="list" aria-label={$t.allParticipants}>{#each event.participants as person}<li><bdi>{person.name}</bdi></li>{/each}</ul>
     <div class="results">{#each event.slots as slot}
      {@const count=voteCounts.get(slot.id)||0}
      <div class="result"><strong>{formatDate(slot.date,$lang)}</strong>
       {#if event.status==='CONFIRMED' && decisiveSlot?.id===slot.id}<span class="tiebreak-badge"><Star size={14} aria-hidden="true"/>{$t.tiebreakBadge}</span>{/if}
       <span>{$plural('availableCount',count)}</span><div class="bar"><i style={`width:${100*count/event.participants.length}%`}></i></div><small>{#each event.participants.filter(p=>p.slots.includes(slot.id)) as person,i}{i?', ':''}<bdi>{person.name}</bdi>{/each}</small>
      </div>
     {/each}</div>
    {/if}
   </section>
   </div>
  </div>
 </div>
{:else}
 <section class="center-card"><h1>{errorCode==='not_found'?$t.pageNotFound:$t.errorTitle}</h1><p class="error" role="alert">{errorText(errorCode||'not_found',$t)}</p>
  <div class="error-actions">
   {#if errorCode.startsWith('service_unavailable')||errorCode==='rate_limited'}<Button onclick={retry}><RotateCw size={16} aria-hidden="true"/>{$t.retry}</Button>{/if}
   <a href={resolve('')}>{$t.createEvent}</a>
  </div>
 </section>
{/if}

<ConfirmDialog bind:this={confirmDialog}/>

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
 .date-option.tiebreak-picked .date-item-icon { background: #e6d6f7; color: #472963; }
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
