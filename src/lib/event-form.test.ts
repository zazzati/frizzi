import { describe, expect, it } from 'vitest';
import { draftOf, draftPayload, emptyDraft } from './event-form.ts';
import type { EventData } from './types.ts';

const event: EventData = {
    title: 'Cena', description: null, location: 'Roma', location_geo: null, organizer_name: 'Ada',
    allow_multiple_choices: false, status: 'OPEN', confirmed_slot_id: null, tiebreak_slot_id: null,
    timezone: 'Europe/Rome', voting_deadline: null, voting_ended: false, participants: [],
    slots: [{ id: 'a', date: '2030-02-03', start: '20:00:00', end: '22:30:00' }, { id: 'b', date: '2030-02-04', start: '19:15:00', end: null }]
};

describe('event drafts', () => {
    it('start empty with one date row', () => {
        const draft = emptyDraft('America/New_York');
        expect(draft).toMatchObject({ title: '', timezone: 'America/New_York', allow_multiple_choices: true, votingDeadline: '' });
        expect(draft.slots).toHaveLength(1);
        expect(emptyDraft().timezone).toBe('Europe/Rome');
    });

    it('edit the saved event with times in hours and minutes', () => {
        const draft = draftOf(event);
        expect(draft).toMatchObject({ title: 'Cena', description: '', location: 'Roma', organizer_name: 'Ada', allow_multiple_choices: false, votingDeadline: '' });
        expect(draft.slots.map(({ key: _key, ...slot }) => slot)).toEqual([
            { id: 'a', date: '2030-02-03', start: '20:00', end: '22:30' },
            { id: 'b', date: '2030-02-04', start: '19:15', end: '' }
        ]);
    });

    it('save as the API payload', () => {
        const payload = draftPayload({ ...draftOf(event), votingDeadline: '2030-02-01' }, 'en');
        expect(Object.keys(payload)).toEqual(['title', 'description', 'location', 'location_geo', 'organizer_name', 'locale', 'allow_multiple_choices', 'timezone', 'voting_deadline', 'slots']);
        expect(payload).toMatchObject({ locale: 'en', location_geo: null, voting_deadline: '2030-02-01' });
        expect(payload.slots[1]).toEqual({ id: 'b', date: '2030-02-04', start: '19:15', end: null });
        expect(draftPayload(emptyDraft(), 'it').voting_deadline).toBeNull();
    });
});
