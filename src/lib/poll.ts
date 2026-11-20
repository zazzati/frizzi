import type { EventData, Slot } from './types.ts';

export type PollState = 'open' | 'closed' | 'confirmed' | 'cancelled';

/** Number of participants available on each slot, keyed by slot id. */
export function countsFor(data: EventData | null | undefined) {
    const counts = new Map<string, number>();
    for (const participant of data?.participants || []) {
        for (const id of participant.slots) counts.set(id, (counts.get(id) || 0) + 1);
    }
    return counts;
}

/** Highest count, or 0 when nobody voted. */
export function maxVotes(counts: Map<string, number>) {
    return Math.max(0, ...counts.values());
}

/** The slot the organizer confirmed, if any. */
export function confirmedSlot(data: EventData | null | undefined): Slot | undefined {
    return data?.slots.find(slot => slot.id === data?.confirmed_slot_id);
}

/** Slots sharing the highest count while the poll is open and someone voted. */
export function tiedLeaders(data: EventData | null | undefined, counts: Map<string, number>): Slot[] {
    const max = maxVotes(counts);
    return data?.status === 'OPEN' && max > 0
        ? data.slots.filter(slot => counts.get(slot.id) === max)
        : [];
}

/**
 * The slot the organizer's tiebreak decided: on a confirmed event, the confirmed slot
 * when it is the tiebreak pick; on an open event, the tiebreak pick when it is one of
 * several tied leaders.
 */
export function decisiveSlot(data: EventData | null | undefined, leaders: Slot[]): Slot | undefined {
    if (data?.status === 'CONFIRMED') {
        const confirmed = confirmedSlot(data);
        return confirmed?.id === data.tiebreak_slot_id ? confirmed : undefined;
    }
    return leaders.length > 1 ? leaders.find(slot => slot.id === data?.tiebreak_slot_id) : undefined;
}

/** Slots shown as leading: the tiebreak pick on an open event, else every tied leader. */
export function leadingSlots(data: EventData | null | undefined, leaders: Slot[], decisive: Slot | undefined): Slot[] {
    return decisive && data?.status === 'OPEN' ? [decisive] : leaders;
}

export function pollState(data: EventData | null | undefined): PollState {
    return data?.status === 'CONFIRMED' ? 'confirmed' : data?.status === 'CANCELLED'
        ? 'cancelled' : data?.voting_ended ? 'closed' : 'open';
}
