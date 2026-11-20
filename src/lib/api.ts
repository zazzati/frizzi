import { resolve } from '$app/paths';
import { track } from './telemetry.ts';
import type { EventData, LocationGeo } from './types.ts';

export type SlotPayload = { id?: string; date: string; start: string; end: string | null };
/** The editable part of an event, as the create and details endpoints take it. */
export type EventPayload = {
 title: string; description: string; location: string; location_geo: LocationGeo | null; organizer_name: string;
 locale: string; allow_multiple_choices: boolean; timezone: string; voting_deadline: string | null; slots: SlotPayload[];
};
export type Participant = { name: string; email: string; slots: string[] };
export type Vote = Participant & { locale: string };
export type Status = 'OPEN' | 'CONFIRMED' | 'CANCELLED';

/** The error code of a failed call, as the page shows it. */
export function errorCode(error: unknown) {
 return error instanceof Error ? error.message : 'error';
}

/**
 * Calls the API below the app's base path. A network failure, a 5xx or an unreadable body throws
 * `service_unavailable`, with the request id appended as a support reference when the API sent one;
 * any other failure throws the API's error code.
 */
export async function request<T = unknown>(url: string, method = 'GET', body?: unknown): Promise<T> {
 const response = await fetch(resolve(url.slice(1)), { method, headers: body ? { 'Content-Type': 'application/json' } : {}, body: body ? JSON.stringify(body) : undefined, cache: 'no-store' }).catch(() => {
  track('browser_network');
  throw new Error('service_unavailable');
 });
 const reference = response.headers.get('x-request-id');
 const support = reference && /^[a-f0-9-]{36}$/.test(reference) ? `:${reference}` : '';
 if (response.status >= 500) {
  if (!reference) track('browser_invalid_response');
  throw new Error(`service_unavailable${support}`);
 }
 const data = await response.json().catch(() => {
  track('browser_invalid_response');
  throw new Error(`service_unavailable${support}`);
 });
 if (!response.ok) throw new Error(typeof data?.error === 'string' ? data.error : 'error');
 return data;
}

const event = (token: string) => `/api/events/${token}`;
const admin = (token: string, secret: string) => `${event(token)}/admin/${secret}`;

export const createEvent = (payload: EventPayload & { organizer_email: string }) =>
 request<{ public_token: string }>('/api/events', 'POST', payload);
export const getEvent = (token: string) => request<EventData>(event(token));
export const getAdminEvent = (token: string, secret: string) => request<EventData>(admin(token, secret));
export const getResponse = (token: string, secret: string) => request<Participant>(`${event(token)}/participant/${secret}`);
export const vote = (token: string, vote: Vote) => request(`${event(token)}/votes`, 'POST', vote);
export const updateResponse = (token: string, secret: string, vote: Vote) =>
 request(`${event(token)}/participant/${secret}`, 'PUT', vote);
export const recoverLink = (token: string, email: string) => request(`${event(token)}/recover`, 'POST', { email });
export const setStatus = (token: string, secret: string, status: Status, confirmed_slot_id: string | null, locale: string) =>
 request(admin(token, secret), 'PUT', { status, confirmed_slot_id, locale });
export const setTiebreak = (token: string, secret: string, slot_id: string | null) =>
 request(`${admin(token, secret)}/tiebreak`, 'PUT', { slot_id });
export const updateDetails = (token: string, secret: string, payload: EventPayload) =>
 request(`${admin(token, secret)}/details`, 'PUT', payload);
export const sendMessage = (token: string, secret: string, message: string, request_id: string) =>
 request<{ queued: number }>(`${admin(token, secret)}/messages`, 'POST', { message, request_id });
/** The iCalendar download of a confirmed event. */
export const calendarHref = (token: string) => resolve(`api/events/${token}/calendar.ics`);
