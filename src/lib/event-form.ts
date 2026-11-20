import { tick } from 'svelte';
import { track } from './telemetry.ts';
import { slotDraft, slotPayload, slotProblems } from './slots.ts';
import { canonicalZone } from './timezones.ts';
import type { EventPayload } from './api.ts';
import type { EventData, LocationGeo, SlotDraft } from './types.ts';

/** What the create form and the organizer's details form edit. */
export type EventDraft = {
 title: string; description: string; location: string; locationGeo: LocationGeo | null; organizer_name: string;
 allow_multiple_choices: boolean; timezone: string; votingDeadline: string; slots: SlotDraft[];
};

/** An empty form with a single date row. */
export function emptyDraft(timezone = 'Europe/Rome'): EventDraft {
 return { title: '', description: '', location: '', locationGeo: null, organizer_name: '', allow_multiple_choices: true, timezone, votingDeadline: '', slots: [slotDraft()] };
}

/** The saved event as a form, with times cut to hours and minutes. */
export function draftOf(event: EventData): EventDraft {
 return {
  title: event.title, description: event.description || '', location: event.location, locationGeo: event.location_geo,
  organizer_name: event.organizer_name, allow_multiple_choices: event.allow_multiple_choices, timezone: event.timezone,
  votingDeadline: event.voting_deadline || '',
  slots: event.slots.map(s => slotDraft({ ...s, start: s.start.slice(0, 5), end: s.end?.slice(0, 5) || '' }))
 };
}

export function draftPayload(draft: EventDraft, locale: string): EventPayload {
 const { title, description, location, locationGeo, organizer_name, allow_multiple_choices, timezone, votingDeadline, slots } = draft;
 return { title, description, location, location_geo: locationGeo, organizer_name, locale, allow_multiple_choices, timezone, voting_deadline: votingDeadline || null, slots: slotPayload(slots) };
}

/**
 * Throws `invalid_input` when a date row or the time zone is wrong. The first wrong row takes
 * focus, after `release` re-enables the form: a disabled field cannot take focus.
 */
export async function checkDraft(draft: EventDraft, release: () => void, creating = false) {
 const found = slotProblems(draft.slots);
 if (found.size) {
  const kinds = new Set(found.values());
  if (creating && kinds.has('duplicateDate')) track('create_duplicate_date');
  if (creating && kinds.has('endBeforeStart')) track('create_time_range');
  const index = draft.slots.findIndex(slot => found.has(slot.key));
  release(); await tick();
  (document.querySelector(found.get(draft.slots[index].key) === 'duplicateDate' ? `#date-${index}` : `[name="end-${index}-hour"]`) as HTMLElement | null)?.focus();
  throw new Error('invalid_input');
 }
 if (!canonicalZone(draft.timezone)) {
  if (creating) track('create_timezone');
  throw new Error('invalid_input');
 }
}
