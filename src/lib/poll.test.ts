import { describe, expect, it } from 'vitest';
import { confirmedSlot, countsFor, decisiveSlot, leadingSlots, maxVotes, pollState, tiedLeaders } from './poll.ts';
import type { EventData, Slot } from './types.ts';

const slot = (id: string, date: string): Slot => ({ id, date, start: '18:00:00', end: null });
const a = slot('a', '2031-04-26');
const b = slot('b', '2031-04-27');
const c = slot('c', '2031-04-28');

function eventWith(overrides: Partial<EventData> = {}): EventData {
    return {
        title: 'Cena', description: null, location: 'Torino', location_geo: null, organizer_name: 'Ada',
        allow_multiple_choices: true, status: 'OPEN', confirmed_slot_id: null, tiebreak_slot_id: null,
        timezone: 'Europe/Rome', voting_deadline: null, voting_ended: false, slots: [a, b, c], participants: [],
        ...overrides
    };
}

const voters = (...choices: string[][]) => choices.map((slots, i) => ({ name: `P${i}`, slots }));

/** Runs the helpers in the same order as the event page. */
function results(data: EventData | null) {
    const counts = countsFor(data);
    const leaders = tiedLeaders(data, counts);
    const decisive = decisiveSlot(data, leaders);
    return { counts, max: maxVotes(counts), leaders, decisive, leading: leadingSlots(data, leaders, decisive), state: pollState(data) };
}

describe('countsFor', () => {
    it('is empty without an event or participants', () => {
        expect(countsFor(null).size).toBe(0);
        expect(countsFor(undefined).size).toBe(0);
        expect(countsFor(eventWith()).size).toBe(0);
    });

    it('counts each participant once per chosen slot', () => {
        const counts = countsFor(eventWith({ participants: voters(['a', 'b'], ['b'], ['b', 'c'], []) }));
        expect(Object.fromEntries(counts)).toEqual({ a: 1, b: 3, c: 1 });
    });

    it('leaves slots nobody chose out of the map', () => {
        expect(countsFor(eventWith({ participants: voters(['a']) })).has('b')).toBe(false);
    });
});

describe('maxVotes', () => {
    it('is zero when nobody voted', () => {
        expect(maxVotes(new Map())).toBe(0);
    });

    it('is the highest count', () => {
        expect(maxVotes(new Map([['a', 2], ['b', 5], ['c', 1]]))).toBe(5);
    });
});

describe('confirmedSlot', () => {
    it('finds the confirmed slot', () => {
        expect(confirmedSlot(eventWith({ status: 'CONFIRMED', confirmed_slot_id: 'b' }))).toBe(b);
    });

    it('is undefined without a confirmation or event', () => {
        expect(confirmedSlot(eventWith())).toBeUndefined();
        expect(confirmedSlot(null)).toBeUndefined();
    });
});

describe('tiedLeaders', () => {
    it('is empty when nobody voted', () => {
        expect(results(eventWith()).leaders).toEqual([]);
        expect(results(eventWith({ participants: voters([]) })).leaders).toEqual([]);
    });

    it('returns the single top slot', () => {
        expect(results(eventWith({ participants: voters(['a', 'b'], ['b']) })).leaders).toEqual([b]);
    });

    it('returns every tied slot in slot order', () => {
        expect(results(eventWith({ participants: voters(['c'], ['a'], ['b']) })).leaders).toEqual([a, b, c]);
        expect(results(eventWith({ participants: voters(['c', 'a'], ['a', 'c'], ['b']) })).leaders).toEqual([a, c]);
    });

    it('counts votes for unknown slot ids toward the maximum (the API never sends them)', () => {
        const data = eventWith({ participants: voters(['gone'], ['gone'], ['a']) });
        expect(results(data).max).toBe(2);
        expect(results(data).leaders).toEqual([]);
    });

    it('is empty once the event is confirmed or cancelled', () => {
        const participants = voters(['a'], ['b']);
        expect(results(eventWith({ status: 'CONFIRMED', confirmed_slot_id: 'a', participants })).leaders).toEqual([]);
        expect(results(eventWith({ status: 'CANCELLED', participants })).leaders).toEqual([]);
    });

    it('still ranks an open poll whose voting ended', () => {
        expect(results(eventWith({ voting_ended: true, participants: voters(['a'], ['a', 'b']) })).leaders).toEqual([a]);
    });

    it('is empty without an event', () => {
        expect(results(null).leaders).toEqual([]);
    });
});

describe('decisiveSlot and leadingSlots', () => {
    const tied = voters(['a', 'c'], ['a', 'c'], ['b']);

    it('show every tied leader before a tiebreak', () => {
        const { decisive, leading } = results(eventWith({ participants: tied }));
        expect(decisive).toBeUndefined();
        expect(leading).toEqual([a, c]);
    });

    it('narrow an open tie to the tiebreak pick', () => {
        const { decisive, leading, leaders } = results(eventWith({ participants: tied, tiebreak_slot_id: 'c' }));
        expect(decisive).toBe(c);
        expect(leading).toEqual([c]);
        expect(leaders).toEqual([a, c]);
    });

    it('ignore a tiebreak pick that is not among the tied leaders', () => {
        const { decisive, leading } = results(eventWith({ participants: tied, tiebreak_slot_id: 'b' }));
        expect(decisive).toBeUndefined();
        expect(leading).toEqual([a, c]);
    });

    it('ignore a tiebreak pick once a single slot leads', () => {
        const { decisive, leading } = results(eventWith({ participants: voters(['a'], ['a', 'c']), tiebreak_slot_id: 'a' }));
        expect(decisive).toBeUndefined();
        expect(leading).toEqual([a]);
    });

    it('mark the confirmed slot when the organizer confirmed the tiebreak pick', () => {
        const { decisive, leading } = results(eventWith({ status: 'CONFIRMED', confirmed_slot_id: 'c', tiebreak_slot_id: 'c', participants: tied }));
        expect(decisive).toBe(c);
        expect(leading).toEqual([]);
    });

    it('mark nothing when the organizer confirmed another slot', () => {
        expect(results(eventWith({ status: 'CONFIRMED', confirmed_slot_id: 'a', tiebreak_slot_id: 'c', participants: tied })).decisive).toBeUndefined();
        expect(results(eventWith({ status: 'CONFIRMED', confirmed_slot_id: 'a', participants: tied })).decisive).toBeUndefined();
    });

    it('mark nothing on a cancelled event', () => {
        const { decisive, leading } = results(eventWith({ status: 'CANCELLED', tiebreak_slot_id: 'c', participants: tied }));
        expect(decisive).toBeUndefined();
        expect(leading).toEqual([]);
    });

    it('mark nothing without an event', () => {
        expect(results(null).decisive).toBeUndefined();
        expect(results(null).leading).toEqual([]);
    });
});

describe('pollState', () => {
    it('is open while voting runs', () => {
        expect(pollState(eventWith())).toBe('open');
    });

    it('is closed once the deadline passed on an open poll', () => {
        expect(pollState(eventWith({ voting_ended: true }))).toBe('closed');
    });

    it('reports confirmation and cancellation before the deadline', () => {
        expect(pollState(eventWith({ status: 'CONFIRMED', confirmed_slot_id: 'a' }))).toBe('confirmed');
        expect(pollState(eventWith({ status: 'CONFIRMED', confirmed_slot_id: 'a', voting_ended: true }))).toBe('confirmed');
        expect(pollState(eventWith({ status: 'CANCELLED' }))).toBe('cancelled');
        expect(pollState(eventWith({ status: 'CANCELLED', voting_ended: true }))).toBe('cancelled');
    });

    it('defaults to open without an event', () => {
        expect(pollState(null)).toBe('open');
    });
});
