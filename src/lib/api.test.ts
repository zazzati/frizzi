import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('$app/paths', () => ({ resolve: (path: string) => `/base/${path}` }));
const track = vi.fn();
vi.mock('./telemetry.ts', () => ({ track }));

const api = await import('./api.ts');
const reference = '0f8b7c3e-1d2a-4b5c-9e6f-7a8b9c0d1e2f';

function respond(status: number, body: string, headers: Record<string, string> = {}) {
    const fetch = vi.fn((_url: string, _init?: RequestInit) => Promise.resolve(new Response(body, { status, headers })));
    vi.stubGlobal('fetch', fetch);
    return fetch;
}

describe('request', () => {
    afterEach(() => { vi.unstubAllGlobals(); track.mockReset(); });

    it('sends JSON below the base path without caching', async () => {
        const fetch = respond(200, '{"queued":2}');
        expect(await api.sendMessage('pub', 'secret', 'Ciao', 'id')).toEqual({ queued: 2 });
        expect(fetch).toHaveBeenCalledWith('/base/api/events/pub/admin/secret/messages', {
            method: 'POST', headers: { 'Content-Type': 'application/json' },
            body: '{"message":"Ciao","request_id":"id"}', cache: 'no-store'
        });
    });

    it('reads without a body', async () => {
        const fetch = respond(200, '{"title":"Cena"}');
        await api.getEvent('pub');
        expect(fetch).toHaveBeenCalledWith('/base/api/events/pub', { method: 'GET', headers: {}, body: undefined, cache: 'no-store' });
    });

    it('throws the API error code', async () => {
        respond(409, '{"error":"email_exists"}');
        await expect(api.vote('pub', { name: 'A', email: 'a@example.test', locale: 'it', slots: [] })).rejects.toThrow('email_exists');
        respond(400, '{"detail":1}');
        await expect(api.getEvent('pub')).rejects.toThrow(/^error$/);
    });

    it('turns server failures into service_unavailable with the support reference', async () => {
        respond(502, '<h1>Bad gateway</h1>', { 'x-request-id': reference });
        await expect(api.getEvent('pub')).rejects.toThrow(`service_unavailable:${reference}`);
        expect(track).not.toHaveBeenCalled();
        respond(503, '');
        await expect(api.getEvent('pub')).rejects.toThrow(/^service_unavailable$/);
        expect(track).toHaveBeenCalledWith('browser_invalid_response');
    });

    it('reports unreadable bodies and network failures', async () => {
        respond(200, 'not json');
        await expect(api.getEvent('pub')).rejects.toThrow(/^service_unavailable$/);
        expect(track).toHaveBeenCalledWith('browser_invalid_response');
        vi.stubGlobal('fetch', vi.fn(() => Promise.reject(new TypeError('Failed to fetch'))));
        await expect(api.getEvent('pub')).rejects.toThrow(/^service_unavailable$/);
        expect(track).toHaveBeenCalledWith('browser_network');
    });
});

describe('endpoints', () => {
    afterEach(() => { vi.unstubAllGlobals(); });

    it('address the organizer, participant and recovery routes', async () => {
        const fetch = respond(200, '{}');
        await api.setStatus('pub', 'adm', 'CONFIRMED', 'slot', 'en');
        await api.setTiebreak('pub', 'adm', null);
        await api.updateResponse('pub', 'own', { name: 'A', email: 'a@example.test', locale: 'it', slots: ['s'] });
        await api.recoverLink('pub', 'a@example.test');
        expect(fetch.mock.calls.map(([url, init]) => `${init?.method} ${url} ${init?.body}`)).toEqual([
            'PUT /base/api/events/pub/admin/adm {"status":"CONFIRMED","confirmed_slot_id":"slot","locale":"en"}',
            'PUT /base/api/events/pub/admin/adm/tiebreak {"slot_id":null}',
            'PUT /base/api/events/pub/participant/own {"name":"A","email":"a@example.test","locale":"it","slots":["s"]}',
            'POST /base/api/events/pub/recover {"email":"a@example.test"}'
        ]);
        expect(api.calendarHref('pub')).toBe('/base/api/events/pub/calendar.ics');
    });

    it('names unknown failures', () => {
        expect(api.errorCode(new Error('rate_limited'))).toBe('rate_limited');
        expect(api.errorCode('boom')).toBe('error');
    });
});
