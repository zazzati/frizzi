import { describe, expect, it } from 'vitest';
import { datesChanged, slotDraft, slotPayload, slotProblems } from './slots.ts';
import type { Slot } from './types.ts';

const saved: Slot[] = [
    { id: 'a', date: '2031-04-25', start: '20:00:00', end: '22:30:00' },
    { id: 'b', date: '2031-04-26', start: '19:30:00', end: null }
];
const drafts = () => saved.map(slot => slotDraft({ ...slot, start: slot.start.slice(0, 5), end: slot.end?.slice(0, 5) || '' }));

describe('slotDraft', () => {
    it('starts empty with a key of its own', () => {
        const [first, second] = [slotDraft(), slotDraft()];
        expect(first).toMatchObject({ date: '', start: '', end: '' });
        expect(first.key).not.toBe(second.key);
    });

    it('keeps the given fields and replaces any key', () => {
        const draft = slotDraft({ id: 'a', date: '2031-04-25', start: '20:00' });
        expect(draft).toMatchObject({ id: 'a', date: '2031-04-25', start: '20:00', end: '' });
        expect(slotDraft(draft).key).toBeGreaterThan(draft.key);
    });
});

describe('slotPayload', () => {
    it('drops the client key and sends an empty end time as null', () => {
        expect(slotPayload([slotDraft({ date: '2031-04-25', start: '20:00' })])).toEqual([{ date: '2031-04-25', start: '20:00', end: null }]);
        expect(slotPayload(drafts())[0]).toEqual({ id: 'a', date: '2031-04-25', start: '20:00', end: '22:30' });
    });
});

describe('datesChanged', () => {
    it('ignores the seconds the API adds and the order of rows', () => {
        expect(datesChanged(saved, drafts())).toBe(false);
        expect(datesChanged(saved, drafts().reverse())).toBe(false);
    });

    it('notices moved, added and removed dates', () => {
        const moved = drafts(); moved[0].date = '2031-04-27';
        const later = drafts(); later[1].end = '23:00';
        expect(datesChanged(saved, moved)).toBe(true);
        expect(datesChanged(saved, later)).toBe(true);
        expect(datesChanged(saved, [...drafts(), slotDraft({ date: '2031-05-01', start: '20:00' })])).toBe(true);
        expect(datesChanged(saved, drafts().slice(1))).toBe(true);
    });
});

describe('slotProblems', () => {
    it('flags the later row of a repeated date and an end not after the start', () => {
        const rows = [slotDraft({ date: '2031-04-25', start: '20:00' }), slotDraft({ date: '2031-04-26', start: '20:00', end: '19:00' }),
            slotDraft({ date: '2031-04-25', start: '18:00' }), slotDraft({ date: '2031-04-27', start: '20:00', end: '20:00' })];
        expect([...slotProblems(rows)]).toEqual([[rows[1].key, 'endBeforeStart'], [rows[2].key, 'duplicateDate'], [rows[3].key, 'endBeforeStart']]);
    });

    it('accepts valid rows, open end times and rows still being filled in', () => {
        expect(slotProblems([slotDraft({ date: '2031-04-25', start: '20:00', end: '22:00' }), slotDraft({ date: '2031-04-26', start: '20:00' }), slotDraft(), slotDraft()]).size).toBe(0);
    });
});
