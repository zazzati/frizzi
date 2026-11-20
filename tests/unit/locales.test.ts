// Mirrors the locale dictionary check in tests/run.mjs so it runs without services.
import { readdirSync, readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const directory = new URL('../../locales/', import.meta.url);
const locales = ['it', 'en', 'es', 'zh', 'hi', 'ar', 'fr', 'bn', 'pt', 'ru', 'de'];
const read = (locale: string) => JSON.parse(readFileSync(new URL(`${locale}.json`, directory), 'utf8'));
const italian = read('it');

describe('locale dictionaries', () => {
    it('has a file for every supported language and no others', () => {
        const files = readdirSync(directory).filter(file => file.endsWith('.json')).map(file => file.slice(0, -5));
        expect(files.sort()).toEqual([...locales].sort());
    });

    describe.each(locales)('%s', locale => {
        const dictionary = read(locale);

        it('has the same UI keys as Italian', () => {
            expect(Object.keys(dictionary.ui).sort()).toEqual(Object.keys(italian.ui).sort());
        });

        it('has the same mail keys as Italian', () => {
            expect(Object.keys(dictionary.mail).sort()).toEqual(Object.keys(italian.mail).sort());
        });

        it('has a non-empty string for every UI key', () => {
            const missing = Object.entries(dictionary.ui).filter(([, value]) => typeof value !== 'string' || !value.length);
            expect(missing.map(([key]) => key)).toEqual([]);
        });
    });
});
