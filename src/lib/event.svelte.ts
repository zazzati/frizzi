import { tick } from 'svelte';
import type { Attachment } from 'svelte/attachments';
import { fromStore } from 'svelte/store';
import { resolve } from '$app/paths';
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
type SlotTime = Pick<Slot, 'date' | 'start' | 'end'>;

/** The slot's time on the viewer's clock, when it differs from the time in the event's zone. */
export function viewerTime(slot: SlotTime, eventZone: string, localZone: string, language: string | null | undefined) {
 if (!localZone) return null;
 const start = convertTime(slot.date, slot.start, eventZone, localZone);
 if (!start || (start.date === slot.date && start.time === slot.start.slice(0, 5))) return null;
 const end = slot.end ? convertTime(slot.date, slot.end, eventZone, localZone) : null;
 return `${start.date !== slot.date ? formatDate(start.date, language) + ', ' : ''}${formatTime(start.time, language)}${end ? ' – ' + formatTime(end.time, language) : ''}`;
}

/**
 * State and actions shared by the guest, participant and organizer pages: loading and refreshing
 * the event, its results, page feedback, the vote draft and the organizer's changes. Responses
 * that arrive after the person moved to other links are dropped.
 */
export class EventPage {
 readonly mode: EventMode;
 event = $state.raw<EventData | null>(null);
 // The server render and the first client render show the loader until the event arrives.
 loading = $state(true);
 refreshing = $state(false);
 busy = $state(false);
 tiebreakSaving = $state(false);
 error = $state('');
 notice = $state<NoticeKey | null>(null);
 scope = $state<FeedbackScope>('page');
 /** The calendar month on show, as YYYY-MM. */
 month = $state('');
 name = $state('');
 email = $state('');
 selected = $state<string[]>([]);
 recoveryOpen = $state(false);
 recoveryEmail = $state('');
 // Read once in the browser: the server render has no viewer zone.
 localZone = $state('');

 counts = $derived(poll.countsFor(this.event));
 maxVotes = $derived(poll.maxVotes(this.counts));
 tiedLeaders = $derived(poll.tiedLeaders(this.event, this.counts));
 decisiveSlot = $derived(poll.decisiveSlot(this.event, this.tiedLeaders));
 leadingSlots = $derived(poll.leadingSlots(this.event, this.tiedLeaders, this.decisiveSlot));
 confirmed = $derived(poll.confirmedSlot(this.event));
 pollState = $derived(poll.pollState(this.event));
 voteFormShown = $derived.by(() => this.mode !== 'admin' && this.event?.status === 'OPEN' && !this.event.voting_ended);
 nearVote = $derived(this.scope === 'vote' && this.voteFormShown);

 #language = fromStore(lang);
 /** Zone names in the page language. */
 eventZone = $derived(this.event ? zoneName(this.event.timezone, intlLocale(this.#language.current)) : '');
 localZoneName = $derived(this.localZone ? zoneName(this.localZone, intlLocale(this.#language.current)) : '');

 #generation = 0;
 #token = '';
 #secret = '';
 #noticeElement: HTMLElement | undefined;

 constructor(mode: EventMode) {
  this.mode = mode;
 }

 /** The viewer's own time for a slot, when it differs from the event's. */
 viewerTime = (slot: SlotTime) =>
  this.event ? viewerTime(slot, this.event.timezone, this.localZone, this.#language.current) : null;

 /** Shows the page for other tokens, from a clean state. */
 open = (token: string, secret = '') => {
  this.#generation++;
  this.#token = token; this.#secret = secret;
  this.month = ''; this.tiebreakSaving = false; this.recoveryOpen = false; this.recoveryEmail = '';
  this.error = ''; this.notice = null; this.scope = 'page'; this.event = null;
  this.name = ''; this.email = ''; this.selected = [];
  return this.#load(this.#generation);
 };

 async #load(current = this.#generation) {
  // The full-page loader only covers the first load; refreshes keep the page mounted.
  this.loading = !this.event; this.refreshing = !!this.event;
  try {
   // A private participant link needs both the event and the saved answer: fetch them together.
   const [data, person] = await Promise.all([
    this.mode === 'admin' ? api.getAdminEvent(this.#token, this.#secret) : api.getEvent(this.#token),
    this.mode === 'edit' ? api.getResponse(this.#token, this.#secret) : undefined
   ]);
   if (current !== this.#generation) return;
   this.event = data;
   if (person) { this.name = person.name; this.email = person.email; this.selected = person.slots; }
   if (!this.localZone) this.localZone = viewerZone();
   // Pick the month on the first load (it is empty) or when the shown month lost its dates.
   if (!data.slots.some(slot => slot.date.startsWith(this.month + '-'))) {
    const confirmedSlot = data.slots.find(slot => slot.id === data.confirmed_slot_id);
    this.month = (confirmedSlot || data.slots.find(slot => this.selected.includes(slot.id)) || data.slots[0])?.date.slice(0, 7) || '';
   }
  } catch (cause) { if (current === this.#generation) this.error = api.errorCode(cause); }
  finally { if (current === this.#generation) { this.loading = false; this.refreshing = false; } }
 }

 retry = () => { this.error = ''; void this.#load(); };

 /** Marks the notice that `#showNotice` focuses. */
 noticeTarget: Attachment<HTMLElement> = node => {
  this.#noticeElement = node;
  return () => { if (this.#noticeElement === node) this.#noticeElement = undefined; };
 };

 // The notice takes focus: the control that triggered the save may be gone or disabled now.
 async #showNotice(key: NoticeKey) {
  this.notice = key; await tick(); this.#noticeElement?.focus();
 }

 /** Page feedback of an action that does not reload the event, such as copying a link. */
 feedback = (code: string) => { this.error = code; this.scope = 'page'; };

 /** Runs a change, reloads the event and confirms it with a notice; failures show their error code. */
 save = async (action: () => Promise<unknown>, key: NoticeKey, where: FeedbackScope = 'page', failed?: (code: string) => void) => {
  this.error = ''; this.notice = null; this.scope = where; this.busy = true;
  try { await action(); await this.#load(); await this.#showNotice(key); }
  catch (cause) { const code = api.errorCode(cause); this.error = code; failed?.(code); }
  finally { this.busy = false; }
 };

 submitVote = (sources: Set<SelectionSource>) => this.save(async () => {
  const vote = { name: this.name, email: this.email, locale: this.#language.current || 'it', slots: this.selected };
  await (this.mode === 'edit' ? api.updateResponse(this.#token, this.#secret, vote) : api.vote(this.#token, vote));
  if (sources.size > 1) track('vote_both');
  else if (sources.has('calendar')) track('vote_calendar');
  else if (sources.has('list')) track('vote_list');
  sources.clear();
  if (this.mode !== 'edit') { this.name = ''; this.email = ''; this.selected = []; }
 }, 'saved', 'vote', code => {
  if (code === 'email_exists') { this.recoveryEmail = this.email; this.recoveryOpen = true; }
  if (code === 'deadline_passed' && this.event) this.event = { ...this.event, voting_ended: true };
 });

 /** Every status change emails all participants: callers confirm it first. */
 setStatus = (status: api.Status, id: string | null, saved: () => void) => this.save(async () => {
  await api.setStatus(this.#token, this.#secret, status, id, this.#language.current || 'it');
  saved();
 }, 'statusSaved');

 setTiebreak = async (slotId: string | null) => {
  if (this.busy || this.tiebreakSaving) return;
  const current = this.#generation;
  this.error = ''; this.notice = null; this.scope = 'page'; this.tiebreakSaving = true;
  try {
   await api.setTiebreak(this.#token, this.#secret, slotId);
   if (current === this.#generation) await this.#load(current);
  } catch (cause) {
   if (current !== this.#generation) return;
   const code = api.errorCode(cause);
   if (code === 'tiebreak_unavailable' || code === 'event_closed') await this.#load(current);
   if (current === this.#generation) this.error = code;
  }
  finally { if (current === this.#generation) this.tiebreakSaving = false; }
 };

 updateDetails = (payload: api.EventPayload) => api.updateDetails(this.#token, this.#secret, payload);
 sendMessage = (message: string, requestId: string) => api.sendMessage(this.#token, this.#secret, message, requestId);
 recover = (address: string) => api.recoverLink(this.#token, address);
 calendarHref = () => api.calendarHref(this.#token);
 publicUrl = () => new URL(resolve(this.#token), window.location.href).href;
 adminUrl = () => new URL(resolve(`${this.#token}/admin/${this.#secret}`), window.location.href).href;
}
