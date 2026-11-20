import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { EventData } from './types.ts';

vi.mock('$app/paths', () => ({ resolve: (path: string) => `/${path}` }));
vi.mock('./telemetry.ts', () => ({ track: vi.fn() }));
const api = vi.hoisted(() => ({
    getEvent: vi.fn(), getAdminEvent: vi.fn(), getResponse: vi.fn(), vote: vi.fn(), updateResponse: vi.fn(),
    setStatus: vi.fn(), setTiebreak: vi.fn()
}));
vi.mock('./api.ts', async original => ({ ...await original<typeof import('./api.ts')>(), ...api }));

const { EventPage, viewerTime } = await import('./event.svelte.ts');

function eventData(changes: Partial<EventData> = {}): EventData {
    return {
        title: 'Cena', description: null, location: 'Roma', location_geo: null, organizer_name: 'Ada',
        allow_multiple_choices: true, status: 'OPEN', confirmed_slot_id: null, tiebreak_slot_id: null,
        timezone: 'Europe/Rome', voting_deadline: null, voting_ended: false,
        slots: [{ id: 'a', date: '2030-02-03', start: '20:00:00', end: null }, { id: 'b', date: '2030-03-04', start: '20:00:00', end: null }],
        participants: [{ name: 'Lucía', slots: ['a', 'b'] }, { name: 'Bo', slots: ['b'] }],
        ...changes
    };
}

describe('EventPage', () => {
    beforeEach(() => { for (const mock of Object.values(api)) mock.mockReset(); });

    it('loads the event with its results and shows the first month', async () => {
        api.getEvent.mockResolvedValue(eventData());
        const view = new EventPage('event');
        expect(view.loading).toBe(true);
        await view.open('pub');
        expect(api.getEvent).toHaveBeenCalledWith('pub');
        expect(view.loading).toBe(false);
        expect(view.counts).toEqual(new Map([['a', 1], ['b', 2]]));
        expect(view.leadingSlots.map(slot => slot.id)).toEqual(['b']);
        expect(view.pollState).toBe('open');
        expect(view.month).toBe('2030-02');
        expect(view.voteFormShown).toBe(true);
    });

    it('fills a participant link with the saved answer and opens on its month', async () => {
        api.getEvent.mockResolvedValue(eventData());
        api.getResponse.mockResolvedValue({ name: 'Bo', email: 'bo@example.test', slots: ['b'] });
        const view = new EventPage('edit');
        await view.open('pub', 'own');
        expect(api.getResponse).toHaveBeenCalledWith('pub', 'own');
        expect([view.name, view.email, view.selected]).toEqual(['Bo', 'bo@example.test', ['b']]);
        expect(view.month).toBe('2030-03');
    });

    it('reads the organizer view, which has no vote form', async () => {
        api.getAdminEvent.mockResolvedValue(eventData({ status: 'CONFIRMED', confirmed_slot_id: 'a' }));
        const view = new EventPage('admin');
        await view.open('pub', 'adm');
        expect(api.getAdminEvent).toHaveBeenCalledWith('pub', 'adm');
        expect(view.confirmed?.id).toBe('a');
        expect(view.voteFormShown).toBe(false);
    });

    it('drops the answer of links the person has left', async () => {
        let first: (data: EventData) => void = () => {};
        api.getEvent.mockReturnValueOnce(new Promise(resolve => first = resolve)).mockResolvedValueOnce(eventData({ title: 'Pranzo' }));
        const view = new EventPage('event');
        const stale = view.open('old');
        await view.open('new');
        first(eventData({ title: 'Cena' }));
        await stale;
        expect(view.event?.title).toBe('Pranzo');
    });

    it('shows load failures as the error code', async () => {
        api.getEvent.mockRejectedValue(new Error('not_found'));
        const view = new EventPage('event');
        await view.open('pub');
        expect([view.event, view.loading, view.error]).toEqual([null, false, 'not_found']);
    });

    it('saves a vote next to the form, clears the guest draft and refreshes', async () => {
        api.getEvent.mockResolvedValue(eventData());
        api.vote.mockResolvedValue({});
        const view = new EventPage('event');
        await view.open('pub');
        view.name = 'Cleo'; view.email = 'cleo@example.test'; view.selected = ['a'];
        await view.submitVote(new Set(['list']));
        expect(api.vote).toHaveBeenCalledWith('pub', { name: 'Cleo', email: 'cleo@example.test', locale: 'it', slots: ['a'] });
        expect([view.name, view.email, view.selected, view.notice, view.scope, view.busy]).toEqual(['', '', [], 'saved', 'vote', false]);
        expect(api.getEvent).toHaveBeenCalledTimes(2);
    });

    it('offers link recovery when the address already voted', async () => {
        api.getEvent.mockResolvedValue(eventData());
        api.vote.mockRejectedValue(new Error('email_exists'));
        const view = new EventPage('event');
        await view.open('pub');
        view.email = 'lucia@example.test';
        await view.submitVote(new Set());
        expect([view.error, view.recoveryOpen, view.recoveryEmail, view.nearVote]).toEqual(['email_exists', true, 'lucia@example.test', true]);
    });

    it('closes the form when the deadline passed meanwhile', async () => {
        api.getEvent.mockResolvedValue(eventData());
        api.vote.mockRejectedValue(new Error('deadline_passed'));
        const view = new EventPage('event');
        await view.open('pub');
        await view.submitVote(new Set());
        expect([view.event?.voting_ended, view.pollState, view.nearVote]).toEqual([true, 'closed', false]);
    });

    it('reloads after a refused tiebreak and keeps the error', async () => {
        api.getAdminEvent.mockResolvedValue(eventData());
        api.setTiebreak.mockRejectedValue(new Error('tiebreak_unavailable'));
        const view = new EventPage('admin');
        await view.open('pub', 'adm');
        await view.setTiebreak('a');
        expect(api.setTiebreak).toHaveBeenCalledWith('pub', 'adm', 'a');
        expect(api.getAdminEvent).toHaveBeenCalledTimes(2);
        expect([view.error, view.tiebreakSaving]).toEqual(['tiebreak_unavailable', false]);
    });
});

describe('viewerTime', () => {
    it('shows the time on the viewer’s clock only when it differs', () => {
        const slot = { date: '2030-02-03', start: '20:00:00', end: '22:00:00' };
        expect(viewerTime(slot, 'Europe/Rome', 'Europe/Rome', 'en')).toBeNull();
        expect(viewerTime(slot, 'Europe/Rome', '', 'en')).toBeNull();
        expect(viewerTime(slot, 'Europe/Rome', 'America/New_York', 'en')).toBe('14:00 – 16:00');
        expect(viewerTime(slot, 'Europe/Rome', 'Asia/Tokyo', 'en')).toMatch(/4 Feb 2030, 4:00 – 6:00$/);
    });
});
