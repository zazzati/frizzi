<script lang="ts">
 import { onDestroy } from 'svelte';
 import { resolve } from '$app/paths';
 import { MapPin } from '@lucide/svelte';
 import { lang, t } from '#lib/i18n.ts';
 import type { LocationGeo } from '#lib/types.ts';
 import LocationAttribution from './LocationAttribution.svelte';

 type Props = { location?: string; locationGeo?: LocationGeo | null };
 let { location = $bindable(''), locationGeo = $bindable(null) }: Props = $props();
 type Suggestion = { label: string; geo: LocationGeo };
 let suggestions = $state<Suggestion[]>([]);
 let active = $state(-1);
 let enabled = true;
 let focused = $state(false);
 let failed = $state(false);
 let timer: ReturnType<typeof setTimeout>;
 let controller: AbortController | undefined;
 const expanded = $derived(focused && suggestions.length > 0);

 function reset() {
   clearTimeout(timer);
   controller?.abort();
   suggestions = [];
   active = -1;
   failed = false;
 }

 onDestroy(reset);

 function input(event: Event) {
   location = (event.currentTarget as HTMLInputElement).value;
   locationGeo = null;
   reset();
   const text = location.trim();
   if (!enabled || [...text].length < 3 || /:\/\//.test(text)) return;
   timer = setTimeout(() => void lookup(text), 350);
 }

 async function lookup(text: string) {
   const request = new AbortController();
   controller = request;
   try {
     const response = await fetch(resolve('api/places/suggest'), {
       method: 'POST',
       headers: { 'Content-Type': 'application/json' },
       body: JSON.stringify({ text, locale: $lang || 'it' }),
       signal: request.signal
     });
     if (!response.ok) throw new Error('location_unavailable');
     const data: { enabled: boolean; suggestions: Suggestion[] } = await response.json();
     if (!request.signal.aborted) {
       enabled = data.enabled;
       suggestions = data.suggestions;
     }
   } catch {
     if (!request.signal.aborted) failed = true;
   }
 }

 function select(result: Suggestion) {
   location = result.label;
   locationGeo = result.geo;
   reset();
 }

 function keydown(event: KeyboardEvent) {
   if (!expanded || event.isComposing) return;
   if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
     event.preventDefault();
     const step = event.key === 'ArrowDown' ? 1 : -1;
     active = active < 0 && step < 0 ? suggestions.length - 1 :
       (active + step + suggestions.length) % suggestions.length;
     document.getElementById(`location-option-${active}`)?.scrollIntoView({ block: 'nearest' });
   } else if (event.key === 'Enter' && active >= 0) {
     event.preventDefault();
     select(suggestions[active]);
   } else if (event.key === 'Escape') {
     event.preventDefault();
     reset();
   }
 }
</script>

<div class="relative min-w-0"
 onfocusout={(event) => {
   if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) focused = false;
 }}>
 <label for="event-location">{$t.location} *</label>
 <input
   id="event-location" name="location" required maxlength="255" autocomplete="off"
   placeholder={$t.locationHint} value={location}
   role="combobox" aria-autocomplete="list" aria-expanded={expanded}
   aria-controls={expanded ? 'location-suggestions' : undefined}
   aria-activedescendant={expanded && active >= 0 ? `location-option-${active}` : undefined}
   oninput={input} onkeydown={keydown}
   onfocus={() => focused = true}
 />
 {#if expanded}
  <div class="location-dropdown">
   <ul id="location-suggestions" role="listbox" aria-label={$t.locationSuggestions}>
    {#each suggestions as result, index}
     <!-- svelte-ignore a11y_click_events_have_key_events (the combobox input handles keyboard selection) -->
     <li id={`location-option-${index}`} role="option" aria-selected={active === index}
       onmousedown={(event) => event.preventDefault()} onclick={() => select(result)}>
      <MapPin size={16} class="shrink-0" aria-hidden="true"/>
      <div>
       {result.label}
       {#if result.geo.kind === 'approximate'}<small>{$t.approximateLocation}</small>{/if}
      </div>
     </li>
    {/each}
   </ul>
   <div class="px-3 pb-2"><LocationAttribution source={suggestions[active < 0 ? 0 : active]?.geo.attribution || null}/></div>
  </div>
 {/if}
 {#if locationGeo?.kind === 'approximate'}
  <p class="fine" role="status">{$t.approximateLocation}</p>
 {/if}
 {#if failed}
  <p class="fine" role="status">{$t.locationSearchUnavailable}</p>
 {/if}
</div>
