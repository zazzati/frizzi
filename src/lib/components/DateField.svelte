<script lang="ts">
 import { CalendarDays } from '@lucide/svelte';
 import { dateFormatter, lang, t } from '#lib/i18n.ts';
 import { dateOrder, datePlaceholder, readDate } from '#lib/dates.ts';

 export let name: string;
 export let label: string;
 export let value = '';
 export let required = false;
 export let describedBy: string | undefined = undefined;
 /** Set by the form when the date is valid on its own but wrong in context, e.g. used twice. */
 export let conflict = false;

 const example = new Date('2031-04-27T12:00:00Z');
 let input: HTMLInputElement;
 let picker: HTMLInputElement;
 let draft = '';
 let syncedValue: string | undefined;
 let syncedFormatter: Intl.DateTimeFormat | undefined;

 $: formatter = dateFormatter($lang);
 $: dateParts = formatter.formatToParts(example);
 $: order = dateOrder(dateParts);
 $: placeholder = datePlaceholder(dateParts, $t.dateLetters);
 $: if (value !== syncedValue || formatter !== syncedFormatter) {
   draft = value ? formatter.format(new Date(`${value}T12:00:00Z`)) : '';
   syncedValue = value;
   syncedFormatter = formatter;
 }
 $: invalid = !!draft.trim() && !value;
 $: input?.setCustomValidity(invalid ? $t.invalidDate : '');
 // The error shows once the person leaves the field or submits, not while typing.
 let touched = false;
 let pressed = false;
 $: showError = invalid && touched;
 // The message pushes the content below it down. Revealing it while the pointer that caused the
 // blur is still pressed would move the target (e.g. Add a date) away and lose the click, so it
 // waits for the release and for the click that follows it.
 function reveal() {
   if (!pressed) return void setTimeout(() => touched = true);
   const later = () => setTimeout(() => touched = true);
   addEventListener('pointerup', later, { once: true });
   addEventListener('pointercancel', later, { once: true });
 }
 $: describedByIds = [describedBy, showError ? `${name}-error` : ''].filter(Boolean).join(' ') || undefined;

 function setDraft(raw: string) {
   draft = raw;
   syncedValue = readDate(raw, order);
   value = syncedValue;
 }

 function usePicker(iso: string) {
   syncedValue = readDate(iso, order);
   value = syncedValue;
   draft = value ? formatter.format(new Date(`${value}T12:00:00Z`)) : '';
 }

 function showCalendar() {
   if (picker.showPicker) {
     try { picker.showPicker(); return; } catch { /* The text field stays available. */ }
   }
   input.focus();
 }
</script>

<svelte:window on:pointerdown|capture={() => pressed = true} on:pointerup|capture={() => pressed = false} on:pointercancel|capture={() => pressed = false}/>

<label for={name}>{label}{required ? ' *' : ''}</label>
<div class="date-entry">
 <input id={name} {name} type="text" inputmode="numeric" autocomplete="off"
  {required} {placeholder} aria-describedby={describedByIds} aria-invalid={showError || conflict} bind:this={input} value={draft}
  on:input={(event) => setDraft(event.currentTarget.value)}
  on:invalid={() => touched = true}
  on:blur={() => { reveal(); if (value) usePicker(value); }}/>
 <button type="button" class="date-picker-button" aria-label={`${$t.openDatePicker}: ${label}`} on:click={showCalendar}>
  <CalendarDays size={17} aria-hidden="true"/>
 </button>
 <input class="native-date" type="date" tabindex="-1" aria-hidden="true" min="0001-01-01" max="9999-12-31"
  value={value} bind:this={picker} on:change={(event) => usePicker(event.currentTarget.value)}/>
</div>
{#if showError}<p class="field-error" id={`${name}-error`}>{$t.invalidDate}</p>{/if}

<style>
 .date-entry { position: relative; }
 .date-entry input[type="text"] { padding-inline-end: 42px; }
 .date-picker-button {
   position: absolute;
   inset-block: 1px;
   inset-inline-end: 1px;
   width: 38px;
   display: grid;
   place-items: center;
   border: 0;
   border-radius: 6px;
   color: #436f49;
   background: transparent;
   cursor: pointer;
 }
 .date-picker-button:hover { background: #edf4e8; }
 .date-entry .native-date {
   position: absolute;
   inset-inline-end: 0;
   inset-block-end: 0;
   margin: 0;
   width: 1px;
   height: 1px;
   padding: 0;
   opacity: 0;
   pointer-events: none;
 }
</style>
