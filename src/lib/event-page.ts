import { tick } from 'svelte';
import { resolve } from '$app/paths';
import { derived, get, writable } from 'svelte/store';
import * as api from './api.ts';
import { formatDate, formatTime, intlLocale, lang } from './i18n.ts';
import * as poll from './poll.ts';
import { track } from './telemetry.ts';
import { convertTime, viewerZone, zoneName } from './timezones.ts';
import type { EventData, Slot } from './types.ts';

/** Who is looking: a guest voting, a participant changing their answer, or the organizer. */
export type EventMode = 'event' | 'edit' | 'admin';
export type NoticeKey = 'saved' | 'statusSaved' | 'changesSaved';
/** Errors and notices of the vote form show next to its button; everything else under the header. */
export type FeedbackScope = 'page' | 'vote';
export type SelectionSource = 'calendar' | 'list';

/** The slot's time on the viewer's clock, when it differs from the time in the event's zone. */
export function viewerTime(slot: Pick<Slot, 'date' | 'start' | 'end'>, eventZone: string, localZone: string, language: string | null | undefined) {
 if (!localZone) return null;
 const start = convertTime(slot.date, slot.start, eventZone, localZone);
 if (!start || (start.date === slot.date && start.time === slot.start.slice(0, 5))) return null;
 const end = slot.end ? convertTime(slot.date, slot.end, eventZone, localZone) : null;
 return `${start.date !== slot.date ? formatDate(start.date, language) + ', ' : ''}${formatTime(start.time, language)}${end ? ' – ' + formatTime(end.time, language) : ''}`;
}

/**
 * State and actions shared by the guest, participant and organizer pages: loading and refreshing
 * the event, page feedback, the vote draft and the organizer's changes. Stale responses of a page
 * the person has navigated away from are dropped.
 */
export function eventPage(mode: EventMode) {
 const event = writable<EventData | null>(null);
 // The server render and the first client render show the loader until the event arrives.
 const loading = writable(true);
 const refreshing = writable(false);
 const busy = writable(false);
 const tiebreakSaving = writable(false);
 const error = writable('');
 const notice = writable<NoticeKey | null>(null);
 const scope = writable<FeedbackScope>('page');
 const month = writable('');
 const name = writable(''), email = writable(''), selected = writable<string[]>([]);
 const recoveryOpen = writable(false), recoveryEmail = writable('');
 // Read once in the browser: the server render has no viewer zone.
 const localZone = writable('');
 let generation = 0, publicToken = '', secret = '';
 let noticeElement: HTMLElement | undefined;

 const results = derived(event, $event => {
  const counts = poll.countsFor($event);
  const tiedLeaders = poll.tiedLeaders($event, counts);
  const decisiveSlot = poll.decisiveSlot($event, tiedLeaders);
  return {
   counts, tiedLeaders, decisiveSlot, maxVotes: poll.maxVotes(counts),
   leadingSlots: poll.leadingSlots($event, tiedLeaders, decisiveSlot),
   confirmed: poll.confirmedSlot($event), pollState: poll.pollState($event)
  };
 });
 const zones = derived([event, localZone, lang], ([$event, $localZone, $lang]) => ({
  event: $event ? zoneName($event.timezone, intlLocale($lang)) : '',
  local: $localZone ? zoneName($localZone, intlLocale($lang)) : '',
  viewerTime: (slot: Pick<Slot, 'date' | 'start' | 'end'>) => $event ? viewerTime(slot, $event.timezone, $localZone, $lang) : null
 }));
 const voteFormShown = derived(event, $event => mode !== 'admin' && $event?.status === 'OPEN' && !$event.voting_ended);
 const nearVote = derived([scope, voteFormShown], ([$scope, $shown]) => $scope === 'vote' && $shown);

 /** Shows the page for other tokens, from a clean state. */
 function open(token: string, privateToken = '') {
  generation++;
  publicToken = token; secret = privateToken;
  month.set(''); tiebreakSaving.set(false); recoveryOpen.set(false); recoveryEmail.set('');
  error.set(''); notice.set(null); scope.set('page'); event.set(null);
  name.set(''); email.set(''); selected.set([]);
  return load(generation);
 }

 async function load(current = generation) {
  // The full-page loader only covers the first load; refreshes keep the page mounted.
  loading.set(!get(event)); refreshing.set(!!get(event));
  try {
   // A private participant link needs both the event and the saved answer: fetch them together.
   const [data, person] = await Promise.all([
    mode === 'admin' ? api.getAdminEvent(publicToken, secret) : api.getEvent(publicToken),
    mode === 'edit' ? api.getResponse(publicToken, secret) : undefined
   ]);
   if (current !== generation) return;
   event.set(data);
   if (person) { name.set(person.name); email.set(person.email); selected.set(person.slots); }
   if (!get(localZone)) localZone.set(viewerZone());
   // Pick the month on the first load (it is empty) or when the shown month lost its dates.
   const shown = get(month);
   if (!data.slots.some(slot => slot.date.startsWith(shown + '-'))) {
    const chosen = get(selected);
    const confirmedSlot = data.slots.find(slot => slot.id === data.confirmed_slot_id);
    month.set((confirmedSlot || data.slots.find(slot => chosen.includes(slot.id)) || data.slots[0])?.date.slice(0, 7) || '');
   }
  } catch (cause) { if (current === generation) error.set(api.errorCode(cause)); }
  finally { if (current === generation) { loading.set(false); refreshing.set(false); } }
 }

 function retry() { error.set(''); void load(); }

 /** Marks the element that `showNotice` focuses, as a `use:` action. */
 function noticeTarget(node: HTMLElement) {
  noticeElement = node;
  return { destroy() { if (noticeElement === node) noticeElement = undefined; } };
 }

 // The notice takes focus: the control that triggered the save may be gone or disabled now.
 async function showNotice(key: NoticeKey) {
  notice.set(key); await tick(); noticeElement?.focus();
 }

 /** Page feedback of an action that does not reload the event, such as copying a link. */
 function feedback(code: string) {
  error.set(code); scope.set('page');
 }

 /** Runs a change, reloads the event and confirms it with a notice; failures show their error code. */
 async function save(action: () => Promise<unknown>, key: NoticeKey, where: FeedbackScope = 'page', failed?: (code: string) => void) {
  error.set(''); notice.set(null); scope.set(where); busy.set(true);
  try { await action(); await load(); await showNotice(key); }
  catch (cause) { const code = api.errorCode(cause); error.set(code); failed?.(code); }
  finally { busy.set(false); }
 }

 function submitVote(sources: Set<SelectionSource>) {
  return save(async () => {
   const vote = { name: get(name), email: get(email), locale: get(lang) || 'it', slots: get(selected) };
   await (mode === 'edit' ? api.updateResponse(publicToken, secret, vote) : api.vote(publicToken, vote));
   if (sources.size > 1) track('vote_both');
   else if (sources.has('calendar')) track('vote_calendar');
   else if (sources.has('list')) track('vote_list');
   sources.clear();
   if (mode !== 'edit') { name.set(''); email.set(''); selected.set([]); }
  }, 'saved', 'vote', code => {
   if (code === 'email_exists') { recoveryEmail.set(get(email)); recoveryOpen.set(true); }
   if (code === 'deadline_passed') event.update(data => data && { ...data, voting_ended: true });
  });
 }

 /** Every status change emails all participants: callers confirm it first. */
 function setStatus(status: api.Status, id: string | null, saved: () => void) {
  return save(async () => {
   await api.setStatus(publicToken, secret, status, id, get(lang) || 'it');
   saved();
  }, 'statusSaved');
 }

 async function setTiebreak(slotId: string | null) {
  if (get(busy) || get(tiebreakSaving)) return;
  const current = generation;
  error.set(''); notice.set(null); scope.set('page'); tiebreakSaving.set(true);
  try {
   await api.setTiebreak(publicToken, secret, slotId);
   if (current === generation) await load(current);
  } catch (cause) {
   if (current !== generation) return;
   const code = api.errorCode(cause);
   if (code === 'tiebreak_unavailable' || code === 'event_closed') await load(current);
   if (current === generation) error.set(code);
  }
  finally { if (current === generation) tiebreakSaving.set(false); }
 }

 return {
  mode, event, loading, refreshing, busy, tiebreakSaving, error, notice, nearVote, month,
  name, email, selected, recoveryOpen, recoveryEmail, results, zones,
  open, retry, noticeTarget, feedback, save, submitVote, setStatus, setTiebreak,
  updateDetails: (payload: api.EventPayload) => api.updateDetails(publicToken, secret, payload),
  sendMessage: (message: string, requestId: string) => api.sendMessage(publicToken, secret, message, requestId),
  recover: (address: string) => api.recoverLink(publicToken, address),
  calendarHref: () => api.calendarHref(publicToken),
  publicUrl: () => new URL(resolve(publicToken), window.location.href).href,
  adminUrl: () => new URL(resolve(`${publicToken}/admin/${secret}`), window.location.href).href
 };
}

export type EventPage = ReturnType<typeof eventPage>;
