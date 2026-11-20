import { describe, expect, it } from 'vitest';
import { dateTimeFormat, numberFormat } from './intl.ts';
import { dateFormatter } from './i18n.ts';

describe('cached Intl formatters', () => {
    it('builds each locale and option set once', () => {
        expect(dateTimeFormat('it-IT', { month: 'short', timeZone: 'UTC' })).toBe(dateTimeFormat('it-IT', { month: 'short', timeZone: 'UTC' }));
        expect(dateTimeFormat('it-IT', { month: 'long', timeZone: 'UTC' })).not.toBe(dateTimeFormat('it-IT', { month: 'short', timeZone: 'UTC' }));
        expect(numberFormat('de-DE')).toBe(numberFormat('de-DE'));
        expect(numberFormat('de-DE', { minimumIntegerDigits: 2 })).not.toBe(numberFormat('de-DE'));
        expect(dateFormatter('it')).toBe(dateFormatter('it'));
    });

    it('formats like a fresh formatter', () => {
        expect(numberFormat('ar-SA').format(1234)).toBe(new Intl.NumberFormat('ar-SA').format(1234));
        expect(dateTimeFormat('en-GB', { dateStyle: 'medium', timeZone: 'UTC' }).format(0)).toBe('1 Jan 1970');
    });

    it('keeps throwing for invalid options instead of caching a failure', () => {
        expect(() => dateTimeFormat('en', { timeZone: 'Mars/Base' })).toThrow(RangeError);
        expect(() => dateTimeFormat('en', { timeZone: 'Mars/Base' })).toThrow(RangeError);
    });
});
