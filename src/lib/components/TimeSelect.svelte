<script lang="ts">
 import { intlLocale, lang, t } from '#lib/i18n.ts';
 import { minuteOptions } from '#lib/dates.ts';
 import { numberFormat } from '#lib/intl.ts';

 type Props = { value?: string; name: string; label: string; required?: boolean; invalid?: boolean; describedBy?: string };
 let { value = $bindable(''), name, label, required = false, invalid = false, describedBy = undefined }: Props = $props();

 const hours = Array.from({ length: 24 }, (_, index) => index.toString().padStart(2, '0'));
 const hour = $derived(value.slice(0, 2));
 const minute = $derived(value.slice(3, 5) || '00');
 const minutes = $derived(minuteOptions(minute));
 const numbers = $derived(numberFormat(intlLocale($lang), { minimumIntegerDigits: 2 }));

 function setHour(hour: string) {
   value = hour ? `${hour}:${minute}` : '';
 }

 function setMinute(minute: string) {
   if (hour) value = `${hour}:${minute}`;
 }
</script>

<fieldset class="time-select">
 <legend>{label}{required ? ' *' : ''}</legend>
 <div class="time-parts">
  <select name={`${name}-hour`} aria-label={`${label}: ${$t.hour}`} {required} aria-invalid={invalid || undefined} aria-describedby={describedBy}
   value={hour} onchange={(event) => setHour(event.currentTarget.value)}>
   <option value="">—</option>
   {#each hours as hourOption}<option value={hourOption}>{numbers.format(Number(hourOption))}</option>{/each}
  </select>
  <span aria-hidden="true">:</span>
  <select name={`${name}-minute`} aria-label={`${label}: ${$t.minute}`} disabled={!hour} aria-invalid={invalid || undefined} aria-describedby={describedBy}
   value={hour ? minute : ''} onchange={(event) => setMinute(event.currentTarget.value)}>
   <option value="" disabled>—</option>
   {#each minutes as minuteOption}<option value={minuteOption}>{numbers.format(Number(minuteOption))}</option>{/each}
  </select>
 </div>
</fieldset>
