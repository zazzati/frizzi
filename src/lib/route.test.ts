import { describe, expect, it } from 'vitest';
import { parsePath } from './route.ts';

const pub = 'AbCdEfGhIjKlMnOpQrS_-';
const secret = 'Zz0123456789_-abcdefg';

describe('parsePath', () => {
    it('creates an event on the home page', () => {
        expect(parsePath(undefined)?.mode).toBe('create');
        expect(parsePath('')?.mode).toBe('create');
    });

    it('reads the public, participant and organizer links', () => {
        expect(parsePath(pub)).toEqual({ mode: 'event', publicToken: pub, secret: '' });
        expect(parsePath(`${pub}/${secret}`)).toEqual({ mode: 'edit', publicToken: pub, secret });
        expect(parsePath(`${pub}/admin/${secret}`)).toEqual({ mode: 'admin', publicToken: pub, secret });
        expect(parsePath(`${pub}/`)?.mode).toBe('event');
    });

    it('rejects anything else', () => {
        for (const path of ['favicon.ico', pub.slice(1), `${pub}x`, `${pub}/admin`, `${pub}/short`, `${pub}/admin/short`,
            `${pub}/other/${secret}`, `${pub}/${secret}/${secret}`, `${pub}/admin/${secret}/extra`, `${pub}/${'a'.repeat(20)}!`])
            expect(parsePath(path), path).toBeNull();
    });
});
