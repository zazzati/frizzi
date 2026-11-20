import type { Slot, SlotDraft } from './types.ts';

let lastKey = 0;

/** A date row of the event form. Its client-only key keeps each row's field state attached to it when rows are removed. */
export function slotDraft(slot: Partial<Omit<SlotDraft, 'key'>> = {}): SlotDraft {
    return { date: '', start: '', end: '', ...slot, key: ++lastKey };
}

/** Date rows as the API expects them: without the client key and with an empty end time as null. */
export function slotPayload(slots: SlotDraft[]) {
    return slots.map(({ key: _key, ...slot }) => ({ ...slot, end: slot.end || null }));
}

/** Whether saving the drafts adds, removes or moves a date, which clears the affected availability. */
export function datesChanged(saved: Slot[], drafts: SlotDraft[]) {
    const row = (slot: { id?: string; date: string; start: string; end: string | null }) =>
        [slot.id || '', slot.date, slot.start.slice(0, 5), slot.end?.slice(0, 5) || ''].join(' ');
    return saved.map(row).sort().join('\n') !== drafts.map(row).sort().join('\n');
}

export type SlotProblem = 'duplicateDate' | 'endBeforeStart';

/** What is wrong with each date row, keyed by row key: a date an earlier row already uses, or an end time not after the start. */
export function slotProblems(slots: SlotDraft[]) {
    const problems = new Map<number, SlotProblem>(), seen = new Set<string>();
    for (const slot of slots) {
        if (slot.date && seen.has(slot.date)) problems.set(slot.key, 'duplicateDate');
        else if (slot.end && slot.start && slot.end <= slot.start) problems.set(slot.key, 'endBeforeStart');
        seen.add(slot.date);
    }
    return problems;
}
