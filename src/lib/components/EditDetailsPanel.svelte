<script lang="ts">
 import { Button } from '#lib/components/ui/button/index.ts';
 import EventFields from './EventFields.svelte';
 import { checkDraft, draftOf, draftPayload, emptyDraft } from '#lib/event-form.ts';
 import { lang, t } from '#lib/i18n.ts';
 import { datesChanged, slotProblems } from '#lib/slots.ts';
 import type { EventPage } from '#lib/event-page.ts';
 import type { EventData, Question } from '#lib/types.ts';

 export let view: EventPage;
 export let event: EventData;
 /** The page's confirmation dialog. */
 export let ask: (question: Question) => Promise<boolean>;
 const { busy } = view;

 let editing = false, slotsChecked = false;
 let draft = emptyDraft();
 $: problems = slotsChecked ? slotProblems(draft.slots) : new Map();

 // Each toggle starts from the saved event.
 function toggle() {
   draft = draftOf(event); slotsChecked = false;
   editing = !editing;
 }

 async function save() {
   if ((draft.timezone!==event.timezone || datesChanged(event.slots,draft.slots))
     && !await ask({title:$t.datesChangedTitle,text:$t.datesChangedHint,confirm:$t.save,cancel:$t.goBack})) return;
   await view.save(async () => {
     slotsChecked = true;
     await checkDraft(draft, () => $busy = false);
     await view.updateDetails(draftPayload(draft, $lang || 'it'));
     editing = false;
   }, 'changesSaved');
 }
</script>

<Button variant="outline" onclick={toggle}>{editing?$t.dismiss:$t.editDetails}</Button>
{#if editing}
 <section class="panel details-panel">
  <form on:submit|preventDefault={save}><fieldset disabled={$busy}>
   <EventFields editing {problems} bind:title={draft.title} bind:description={draft.description} bind:location={draft.location} bind:locationGeo={draft.locationGeo}
    bind:slots={draft.slots} bind:allow_multiple_choices={draft.allow_multiple_choices} bind:timezone={draft.timezone} bind:votingDeadline={draft.votingDeadline}/>
   <label class="mt-5">{$t.organized}<input name="organizer_name" required maxlength="100" bind:value={draft.organizer_name}/></label>
   <Button type="submit" class="submit" disabled={$busy}>{$t.save}</Button>
  </fieldset></form>
 </section>
{/if}
