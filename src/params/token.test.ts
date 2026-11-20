import { describe, expect, it } from 'vitest';
import { params } from './index.ts';
import { token } from './token.ts';

const pub = 'AbCdEfGhIjKlMnOpQrS_-';
const secret = 'Zz0123456789_-abcdefg';

describe('token matcher', () => {
    it('accepts public, participant and organizer tokens', () => {
        for (const value of [pub, secret, 'a'.repeat(21), '-'.repeat(21)]) expect(token(value), value).toBe(value);
    });

    it('rejects anything else', () => {
        for (const param of ['', 'admin', 'privacy', 'favicon.svg', 'not-an-event', pub.slice(1), `${pub}x`,
            `${'a'.repeat(20)}!`, `${'a'.repeat(20)}.`, `${'a'.repeat(20)}/`, `${pub}\n`, ' '.repeat(21)])
            expect(token(param), JSON.stringify(param)).toBeUndefined();
    });

    it('is registered for routes as [name=token]', () => {
        expect(params.token['~standard'].validate(pub)).toEqual({ value: pub });
        expect(params.token['~standard'].validate('admin')).toHaveProperty('issues');
    });
});
