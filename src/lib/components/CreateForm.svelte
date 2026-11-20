<script lang="ts">
 import { onMount } from 'svelte';
 import { afterNavigate } from '$app/navigation';
 import { resolve } from '$app/paths';
 import { Button } from '#lib/components/ui/button/index.ts';
 import EventFields from './EventFields.svelte';
 import { createEvent, errorCode as codeOf } from '#lib/api.ts';
 import { checkDraft, draftPayload, emptyDraft } from '#lib/event-form.ts';
 import { errorText, lang, t } from '#lib/i18n.ts';
 import { slotProblems } from '#lib/slots.ts';
 import { track } from '#lib/telemetry.ts';
 import { viewerZone } from '#lib/timezones.ts';
 import { onSubmit } from '#lib/utils.ts';

 /** `created` receives the public link and the title of the new event. */
 let { created }: { created: (link: string, title: string) => void } = $props();

 let draft = $state(emptyDraft()), organizer_email = $state('');
 let busy = $state(false), errorCode = $state(''), slotsChecked = $state(false);
 // From the first attempt on, each row shows what is wrong with it and clears as it is fixed.
 const problems = $derived(slotsChecked ? slotProblems(draft.slots) : new Map());

 // The home link starts over, also when it is the page already shown.
 function reset() {
   draft = emptyDraft(viewerZone()); organizer_email = '';
   errorCode = ''; slotsChecked = false;
 }
 onMount(() => { draft.timezone = viewerZone(); });
 afterNavigate(reset);

 async function create() {
   errorCode = ''; busy = true;
   try {
     slotsChecked = true;
     await checkDraft(draft, () => busy = false, true);
     const data = await createEvent({ ...draftPayload(draft, $lang || 'it'), organizer_email });
     track(draft.locationGeo ? 'location_suggested' : 'location_manual');
     created(new URL(resolve(data.public_token), window.location.href).href, draft.title);
   }
   catch (error) { errorCode = codeOf(error); }
   finally { busy = false; }
 }
</script>

<div class="party-scene party-scene--create">
 <div class="create-heading">
  <h1>{$t.create}</h1>
  <div class="party-guests party-guests--trio" aria-hidden="true"></div>
 </div>
 <form id="form" onsubmit={onSubmit(create)} class="form-shell party-card">
 <fieldset disabled={busy}>
  <EventFields {problems} bind:title={draft.title} bind:description={draft.description} bind:location={draft.location} bind:locationGeo={draft.locationGeo}
   bind:slots={draft.slots} bind:allow_multiple_choices={draft.allow_multiple_choices} bind:timezone={draft.timezone} bind:votingDeadline={draft.votingDeadline}/>
  <div class="divider"></div>
  <div class="section-heading"><div><h2>{$t.organizer}</h2><p>{$t.privacy} <a href={resolve('/privacy')}>{$t.privacyTitle}</a></p></div></div>
  <div class="form-grid">
   <label>{$t.name} *<input name="organizer_name" required maxlength="100" autocomplete="name" bind:value={draft.organizer_name}/></label>
   <label>{$t.email} *<input name="organizer_email" type="email" required maxlength="255" autocomplete="email" bind:value={organizer_email}/></label>
  </div>
  {#if errorCode}<p class="error" role="alert">{errorText(errorCode,$t)}</p>{/if}
  <Button type="submit" class="submit" disabled={busy}>{busy?$t.creating:$t.submit}</Button>
 </fieldset>
 </form>
</div>
