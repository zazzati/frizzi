import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('$app/paths', () => ({ resolve: (path: string) => `/base/${path}` }));

const { track } = await import('./telemetry.ts');

describe('track', () => {
    afterEach(() => { vi.unstubAllGlobals(); });

    it('posts the interaction without credentials or referrer', () => {
        const fetch = vi.fn(() => Promise.resolve(new Response(null, { status: 204 })));
        vi.stubGlobal('fetch', fetch);
        track('share_copy');
        expect(fetch).toHaveBeenCalledWith('/base/api/telemetry', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: '"share_copy"',
            credentials: 'omit',
            referrerPolicy: 'no-referrer',
            keepalive: true
        });
    });

    it('ignores network failures', async () => {
        // Vitest fails the run on an unhandled rejection, so settling quietly is the assertion.
        const fetch = vi.fn(() => Promise.reject(new TypeError('Failed to fetch')));
        vi.stubGlobal('fetch', fetch);
        expect(() => track('browser_network')).not.toThrow();
        await new Promise(resolve => setTimeout(resolve, 0));
        expect(fetch).toHaveBeenCalledOnce();
    });
});
