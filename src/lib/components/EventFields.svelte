<script lang="ts">
 import { tick } from 'svelte';
 import { Plus, X } from '@lucide/svelte';
 import { formatDate, intlLocale, lang, t } from '#lib/i18n.ts';
 import LocationField from './LocationField.svelte';
 import TimeSelect from './TimeSelect.svelte';
 import DateField from './DateField.svelte';
 import { slotDraft, type SlotProblem } from '#lib/slots.ts';
 import { canonicalZone, zoneDetails, zoneOptions } from '#lib/timezones.ts';
 import type { LocationGeo, SlotDraft } from '#lib/types.ts';

 type Props = {
   title?: string; description?: string; location?: string; locationGeo?: LocationGeo | null;
   slots?: SlotDraft[]; allow_multiple_choices?: boolean; timezone?: string; editing?: boolean; votingDeadline?: string;
   /** Rows the last save attempt rejected, keyed by row key; they update as the rows are fixed. */
   problems?: Map<number, SlotProblem>;
 };
 let {
   title = $bindable(''), description = $bindable(''), location = $bindable(''), locationGeo = $bindable(null),
   slots = $bindable([slotDraft()]), allow_multiple_choices = $bindable(true), timezone = $bindable('Europe/Rome'),
   editing = false, votingDeadline = $bindable(''), problems = new Map()
 }: Props = $props();
 let addButton: HTMLButtonElement, zoneInput: HTMLInputElement;
 // The zone list is built on first focus: hundreds of labels are only worth it when someone opens it.
 let listedFor = $state<string | null>(null), zoneTouched = $state(false);
 const locale = $derived(intlLocale($lang));
 const zoneList = $derived(listedFor === null ? [] : zoneOptions(locale, listedFor));
 const zone = $derived(canonicalZone(timezone));
 $effect(() => zoneInput.setCustomValidity(zone ? '' : $t.invalidTimezone));
 const zoneHint = $derived(zone ? zoneDetails(zone, locale) : '');

 async function addDate() {
   const previous = slots.at(-1);
   let date = '';
   if (previous?.date) {
     // Date inputs hold calendar days. UTC keeps the increment independent of DST.
     const next = new Date(`${previous.date}T00:00:00Z`);
     next.setUTCDate(next.getUTCDate() + 1);
     if (Number.isFinite(next.getTime()) && next.getUTCFullYear() <= 9999) {
       date = next.toISOString().slice(0, 10);
     }
   }
   slots = [...slots, slotDraft({ date, start: previous?.start || '', end: previous?.end || '' })];
   await tick();
   document.getElementById(`date-${slots.length - 1}`)?.focus();
 }

 // Focus moves to the previous row's remove button, or to Add when a single row is left.
 async function removeDate(index: number) {
   slots = slots.filter((_, other) => other !== index);
   await tick();
   (slots.length > 1 && document.getElementById(`remove-${slots[Math.max(0, index - 1)].key}`) || addButton)?.focus();
 }
</script>
<div class="section-heading"><h2>{$t.detail}</h2><span class="required">* {$t.required}</span></div>
<div class="form-grid">
 <label>{$t.title} *<input name="title" required maxlength="255" bind:value={title} placeholder={$t.titleHint}/></label>
 <LocationField bind:location bind:locationGeo/>
 <label class="wide">{$t.description} <span>({$t.optional})</span><textarea name="description" maxlength="10000" bind:value={description} rows="2"></textarea></label>
</div>
<div class="divider"></div>
<div class="section-heading"><div><h2>{$t.dates}</h2><p>{editing?$t.datesChangedHint:$t.datesText}</p></div></div>
{#each slots as slot,i (slot.key)}
 {@const problem=problems.get(slot.key)}
 {@const errorId=problem?`slot-error-${slot.key}`:undefined}
 <div class="slot-row" class:removable={slots.length>1}>
 <div class="slot-date"><DateField name={`date-${i}`} label={$t.date} required conflict={problem==='duplicateDate'} describedBy={problem==='duplicateDate'?errorId:undefined} bind:value={slot.date}/></div>
 <div class="slot-times">
  <TimeSelect name={`start-${i}`} label={$t.start} required bind:value={slot.start}/>
  <TimeSelect name={`end-${i}`} label={$t.end} invalid={problem==='endBeforeStart'} describedBy={problem==='endBeforeStart'?errorId:undefined} bind:value={slot.end}/>
 </div>
 {#if slots.length>1}<button type="button" class="remove" id={`remove-${slot.key}`} aria-label={`${$t.remove} · ${slot.date?formatDate(slot.date,$lang):`${$t.date} ${i+1}`}`} onclick={()=>removeDate(i)}><X size={18} aria-hidden="true"/></button>{/if}
 {#if problem}<p class="field-error slot-error" id={errorId}>{$t[problem]}</p>{/if}
</div>{/each}
<button class="text-button icon-button" type="button" disabled={slots.length>=100} bind:this={addButton} onclick={addDate}><Plus size={17} aria-hidden="true"/>{$t.add}</button>
<label class="check"><input name="multiple" type="checkbox" bind:checked={allow_multiple_choices}/>{$t.multiple}</label>
<div class="form-grid">
 <div>
  <label>{$t.timezone}<input name="timezone" required maxlength="64" list="timezone-options" autocomplete="off" spellcheck="false"
   aria-describedby="timezone-hint" aria-invalid={zoneTouched && !zone} bind:this={zoneInput} bind:value={timezone}
   onfocus={()=>{if(listedFor===null)listedFor=timezone;}} oninvalid={()=>zoneTouched=true} onchange={()=>{if(zone)timezone=zone;}}/></label>
  {#if zoneList.length}<datalist id="timezone-options">{#each zoneList as option (option.zone)}<option value={option.zone}>{option.label}</option>{/each}</datalist>{/if}
  <small class="block mt-1" class:fine={zone || !zoneTouched} class:field-error={!zone && zoneTouched} id="timezone-hint">{zone ? zoneHint : zoneTouched ? $t.invalidTimezone : $t.timezoneHint}</small>
 </div>
 <div>
  <DateField name="voting_deadline" label={`${$t.votingDeadline} (${$t.optional})`} describedBy="deadline-hint" bind:value={votingDeadline}/>
  <small class="fine block mt-1" id="deadline-hint">{$t.deadlineHint}</small>
 </div>
</div>
