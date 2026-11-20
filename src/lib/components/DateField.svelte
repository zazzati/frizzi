<script lang="ts">
 import { CalendarDays } from '@lucide/svelte';
 import { dateFormatter, lang, t } from '#lib/i18n.ts';
 import { dateOrder, datePlaceholder, readDate } from '#lib/dates.ts';

 type Props = {
   name: string;
   label: string;
   value?: string;
   required?: boolean;
   describedBy?: string;
   /** Set by the form when the date is valid on its own but wrong in context, e.g. used twice. */
   conflict?: boolean;
 };
 let { name, label, value = $bindable(''), required = false, describedBy = undefined, conflict = false }: Props = $props();

 const example = new Date('2031-04-27T12:00:00Z');
 let input: HTMLInputElement;
 let picker: HTMLInputElement;

 const formatter = $derived(dateFormatter($lang));
 const dateParts = $derived(formatter.formatToParts(example));
 const order = $derived(dateOrder(dateParts));
 const placeholder = $derived(datePlaceholder(dateParts, $t.dateLetters));
 // What the person typed, kept while it still reads as the current value in the current format;
 // any other value, from the picker or the form, shows formatted.
 let typed = $state<{ raw: string; value: string; formatter: Intl.DateTimeFormat } | null>(null);
 const draft = $derived(typed && typed.value === value && typed.formatter === formatter ? typed.raw
   : value ? formatter.format(new Date(`${value}T12:00:00Z`)) : '');
 const invalid = $derived(!!draft.trim() && !value);
 $effect(() => input.setCustomValidity(invalid ? $t.invalidDate : ''));
 // The error shows once the person leaves the field or submits, not while typing.
 let touched = $state(false);
 let pressed = false;
 const showError = $derived(invalid && touched);
 // The message pushes the content below it down. Revealing it while the pointer that caused the
 // blur is still pressed would move the target (e.g. Add a date) away and lose the click, so it
 // waits for the release and for the click that follows it.
 function reveal() {
   if (!pressed) return void setTimeout(() => touched = true);
   const later = () => setTimeout(() => touched = true);
   addEventListener('pointerup', later, { once: true });
   addEventListener('pointercancel', later, { once: true });
 }
 const describedByIds = $derived([describedBy, showError ? `${name}-error` : ''].filter(Boolean).join(' ') || undefined);

 function setDraft(raw: string) {
   const next = readDate(raw, order);
   typed = { raw, value: next, formatter };
   value = next;
 }

 function usePicker(iso: string) {
   typed = null;
   value = readDate(iso, order);
 }

 function showCalendar() {
   if (picker.showPicker) {
     try { picker.showPicker(); return; } catch { /* The text field stays available. */ }
   }
   input.focus();
 }
</script>

<svelte:window onpointerdowncapture={() => pressed = true} onpointerupcapture={() => pressed = false} onpointercancelcapture={() => pressed = false}/>

<label for={name}>{label}{required ? ' *' : ''}</label>
<div class="date-entry">
 <input id={name} {name} type="text" inputmode="numeric" autocomplete="off"
  {required} {placeholder} aria-describedby={describedByIds} aria-invalid={showError || conflict} bind:this={input} value={draft}
  oninput={(event) => setDraft(event.currentTarget.value)}
  oninvalid={() => touched = true}
  onblur={() => { reveal(); if (value) usePicker(value); }}/>
 <button type="button" class="date-picker-button" aria-label={`${$t.openDatePicker}: ${label}`} onclick={showCalendar}>
  <CalendarDays size={17} aria-hidden="true"/>
 </button>
 <input class="native-date" type="date" tabindex="-1" aria-hidden="true" min="0001-01-01" max="9999-12-31"
  value={value} bind:this={picker} onchange={(event) => usePicker(event.currentTarget.value)}/>
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
