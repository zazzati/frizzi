import { afterEach, describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { get } from 'svelte/store';
import { dateFormatter, detectLocale, errorText, formatDate, formatTime, intlLocale, lang, languages, plural, pluralKeys, t, type PluralKey } from './i18n.ts';
import it_ from '../../locales/it.json';
import en from '../../locales/en.json';

const ui = it_.ui;
const reference = '0f8fad5b-d9cb-41a0-a1b2-0c1d2e3f4a5b';

describe('errorText', () => {
    it('maps every API error code to its message', () => {
        const codes: Record<string, keyof typeof ui> = {
            invalid_input: 'invalidInput', email_exists: 'emailExists', not_found: 'notFound',
            multiple_votes_exist: 'multipleVotesExist', rate_limited: 'rateLimited', event_closed: 'eventClosed',
            invalid_deadline: 'invalidDeadline', deadline_passed: 'deadlinePassed',
            tiebreak_unavailable: 'tiebreakUnavailable', service_unavailable: 'serviceUnavailable'
        };
        for (const [code, key] of Object.entries(codes)) expect(errorText(code, ui), code).toBe(ui[key]);
    });

    it('falls back to the generic message for unknown codes', () => {
        expect(errorText('error', ui)).toBe(ui.error);
        expect(errorText('something_new', ui)).toBe(ui.error);
        expect(errorText('', ui)).toBe(ui.error);
    });

    it('ignores codes inherited from Object.prototype', () => {
        for (const code of ['toString', 'constructor', '__proto__', 'hasOwnProperty', 'valueOf'])
            expect(errorText(code, ui), code).toBe(ui.error);
    });

    it('has a message for a failed copy to the clipboard', () => {
        expect(errorText('copy_failed', ui)).toBe(ui.copyFailed);
    });

    it('appends a support code when the reference is a request id', () => {
        expect(errorText(`service_unavailable:${reference}`, ui))
            .toBe(`${ui.serviceUnavailable} ${ui.supportCode} ${reference}`);
        expect(errorText(`not_found:${reference}`, ui)).toBe(`${ui.notFound} ${ui.supportCode} ${reference}`);
        expect(errorText(`unknown:${reference}`, ui)).toBe(`${ui.error} ${ui.supportCode} ${reference}`);
    });

    it('drops references that are not request ids', () => {
        expect(errorText('service_unavailable:abc', ui)).toBe(ui.serviceUnavailable);
        expect(errorText(`service_unavailable:${reference.toUpperCase()}`, ui)).toBe(ui.serviceUnavailable);
        expect(errorText(`service_unavailable:${reference}0`, ui)).toBe(ui.serviceUnavailable);
        expect(errorText(`service_unavailable:${reference}:extra`, ui)).toBe(`${ui.serviceUnavailable} ${ui.supportCode} ${reference}`);
    });

    it('uses the messages it is given', () => {
        expect(errorText('not_found', en.ui)).toBe(en.ui.notFound);
    });
});

describe('t', () => {
    afterEach(async () => { await lang.set('it'); });

    it('starts in Italian with every UI key except the plural messages', () => {
        const messages = get(t);
        expect(get(lang)).toBe('it');
        expect(Object.keys(messages).sort()).toEqual(Object.keys(ui).filter(key => !(pluralKeys as readonly string[]).includes(key)).sort());
        expect(messages.notFound).toBe(ui.notFound);
    });

    it('loads and follows the selected language', async () => {
        await lang.set('en');
        expect(get(t).notFound).toBe(en.ui.notFound);
    });

    it('lists the eleven supported languages', () => {
        expect(Object.keys(languages)).toEqual(['it', 'en', 'es', 'zh', 'hi', 'ar', 'fr', 'bn', 'pt', 'ru', 'de']);
    });
});

describe('plural', () => {
    const dictionaries = import.meta.glob<typeof it_>('../../locales/*.json', { eager: true, import: 'default' });
    const uiFor = (locale: string) => dictionaries[`../../locales/${locale}.json`].ui;
    afterEach(async () => { await lang.set('it'); });

    it.each<[string, PluralKey, number, Record<string, number>, string]>([
        ['it', 'availableCount', 1, {}, '1 disponibile'], ['it', 'availableCount', 3, {}, '3 disponibili'],
        ['it', 'availableOf', 1, { total: 4 }, '1 / 4 disponibile'], ['it', 'availableOf', 0, { total: 4 }, '0 / 4 disponibili'],
        ['en', 'messageQueued', 1, {}, '1 email queued'], ['en', 'messageQueued', 2, {}, '2 emails queued'],
        ['fr', 'availableCount', 0, {}, '0 disponible'], ['pt', 'availableCount', 0, {}, '0 disponíveis'], ['pt', 'availableCount', 1, {}, '1 disponível'],
        ['ru', 'messageQueued', 1, {}, '1 письмо в очереди'], ['ru', 'messageQueued', 3, {}, '3 письма в очереди'],
        ['ru', 'messageQueued', 5, {}, '5 писем в очереди'], ['ru', 'messageQueued', 21, {}, '21 письмо в очереди'],
        ['ar', 'availableCount', 0, {}, 'لا أحد متاح'], ['ar', 'availableCount', 1, {}, 'شخص واحد متاح'], ['ar', 'availableCount', 2, {}, 'شخصان متاحان'],
        ['ar', 'availableCount', 3, {}, '٣ أشخاص متاحون'], ['ar', 'availableCount', 11, {}, '١١ شخصًا متاحًا'], ['ar', 'availableCount', 100, {}, '١٠٠ شخص متاح'],
        ['ar', 'availableOf', 3, { total: 4 }, '٣ / ٤ متاحون'], ['bn', 'availableCount', 3, {}, '৩ জন আসতে পারবেন'],
        ['de', 'availableOf', 1000, { total: 1200 }, '1.000 / 1.200 verfügbar'], ['zh', 'availableCount', 1, {}, '1 人有空']
    ])('%s %s %d', async (locale, key, count, values, expected) => {
        await lang.set(locale);
        expect(get(plural)(key, count, values)).toBe(expected);
    });

    it('has every plural form each language uses for whole numbers', () => {
        for (const locale of Object.keys(languages)) {
            const rules = new Intl.PluralRules(locale);
            const needed = new Set(['other', ...Array.from({ length: 1000 }, (_, n) => rules.select(n))]);
            for (const key of pluralKeys) for (const form of needed)
                expect(uiFor(locale)[key], `${locale} ${key} ${form}`).toMatch(new RegExp(`[\\s,]${form} \\{`));
        }
    });

    it('formats every message without leftover ICU syntax', async () => {
        for (const locale of Object.keys(languages)) {
            await lang.set(locale);
            for (const key of pluralKeys) for (const count of [0, 1, 2, 3, 5, 11, 21, 102, 1001])
                expect(get(plural)(key, count, { total: count + 1 }), `${locale} ${key} ${count}`).not.toMatch(/[{}#]|^ui\./);
        }
    });
});

describe('detectLocale', () => {
    const cases: [string | null, string[], string][] = [
        [null, ['de-DE', 'en'], 'de'], ['fr', ['de-DE'], 'fr'], ['xx', ['pt-BR'], 'pt'], [null, ['ZH-Hant-TW'], 'zh'],
        [null, ['en_US'], 'en'], [null, ['nl-NL', 'sv', 'ar-EG'], 'ar'], [null, ['nl-NL', 'sv'], 'it'], [null, [], 'it'],
        ['toString', ['__proto__', 'constructor'], 'it']
    ];

    it('prefers the saved language, then the first supported browser language, then Italian', () => {
        for (const [saved, preferred, expected] of cases) expect(detectLocale(saved, preferred), `${saved} ${preferred}`).toBe(expected);
    });

    it('matches the inline script that sets lang and dir before the first paint', () => {
        const html = readFileSync(new URL('../app.html', import.meta.url), 'utf8');
        const script = html.match(/<script>(\/\* Language and direction[\s\S]*?)<\/script>/)![1];
        for (const [saved, preferred, expected] of cases) {
            const root = { lang: '', dir: '' };
            new Function('document', 'localStorage', 'navigator', script)({ documentElement: root }, { getItem: () => saved }, { languages: preferred });
            expect(root, `${saved} ${preferred}`).toEqual({ lang: expected, dir: expected === 'ar' ? 'rtl' : 'ltr' });
        }
        const root = { lang: '', dir: '' };
        new Function('document', 'localStorage', 'navigator', script)({ documentElement: root }, { getItem: () => { throw new Error('blocked'); } }, { languages: ['ar'] });
        expect(root).toEqual({ lang: 'ar', dir: 'rtl' });
    });
});

describe('date and time formatting', () => {
    it('formats dates in UTC with the Gregorian calendar', () => {
        expect(dateFormatter('it').format(new Date('2031-04-27T00:30:00Z'))).toBe('27/04/2031');
        expect(dateFormatter('de').format(new Date('2031-04-27T23:30:00Z'))).toBe('27.04.2031');
        expect(dateFormatter('zh').format(new Date('2031-04-27T12:00:00Z'))).toBe('2031/04/27');
        expect(dateFormatter('ar').resolvedOptions().calendar).toBe('gregory');
    });

    it('falls back to Italian for unknown or missing languages', () => {
        const italian = dateFormatter('it').resolvedOptions().locale;
        expect(dateFormatter('xx').resolvedOptions().locale).toBe(italian);
        expect(dateFormatter(null).resolvedOptions().locale).toBe(italian);
        expect(dateFormatter(undefined).resolvedOptions().locale).toBe(italian);
    });

    it('spells out the weekday and month for reading', () => {
        expect(formatDate('2030-02-03', 'it')).toBe('dom 3 feb 2030');
        expect(formatDate('2031-04-27', 'en')).toBe('Sun, 27 Apr 2031');
        expect(formatDate('2031-04-27', 'de')).toBe('So., 27. Apr. 2031');
        expect(formatDate('2030-02-03', 'ar')).toBe('الأحد، ٣ فبراير ٢٠٣٠');
    });

    it('never prints a numeric month, Portuguese included', () => {
        for (const language of Object.keys(languages)) expect(formatDate('2030-02-03', language), language).not.toMatch(/[\/.-]0?2[\/.-]/);
        expect(formatDate('2030-02-03', 'pt')).toContain('fevereiro');
    });

    it('formats the date in UTC so it never shifts a day', () => {
        expect(formatDate('2030-12-31', 'it')).toBe('mar 31 dic 2030');
    });

    it('formats times without shifting them', () => {
        expect(formatTime('18:30', 'it')).toBe('18:30');
        expect(formatTime('09:05:00', 'it')).toBe('9:05');
        expect(formatTime('18:30', null)).toBe('18:30');
    });

    it('formats times with the same regional locale as dates', () => {
        expect(intlLocale('en')).toBe('en-GB');
        expect(intlLocale('pt')).toBe('pt-PT');
        expect(intlLocale('xx')).toBe('it-IT');
        expect(intlLocale('toString')).toBe('it-IT');
        expect(formatTime('18:30', 'en')).toBe('18:30');
    });
});
