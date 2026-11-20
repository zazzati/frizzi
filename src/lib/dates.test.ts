import { describe, expect, it } from 'vitest';
import { dateOrder, minuteOptions, datePlaceholder, firstWeekday, readDate, type DatePart } from './dates.ts';
import { dateFormatter, intlLocale, languages } from './i18n.ts';

const example = new Date('2031-04-27T12:00:00Z');
const orderFor = (language: string) => dateOrder(dateFormatter(language).formatToParts(example));
const dmy: DatePart[] = ['day', 'month', 'year'];
const ymd: DatePart[] = ['year', 'month', 'day'];
const mdy: DatePart[] = ['month', 'day', 'year'];

describe('dateOrder', () => {
    it('follows the field order of each supported language', () => {
        for (const language of Object.keys(languages)) {
            expect(orderFor(language), language).toEqual(language === 'zh' ? ymd : dmy);
        }
    });

    it('ignores literals and other parts', () => {
        expect(dateOrder(new Intl.DateTimeFormat('de-DE', { weekday: 'long', day: '2-digit', month: '2-digit', year: 'numeric', timeZone: 'UTC' }).formatToParts(example))).toEqual(dmy);
    });
});

describe('datePlaceholder', () => {
    const dictionaries = import.meta.glob<{ ui: { dateLetters: string } }>('../../locales/*.json', { eager: true, import: 'default' });
    const placeholder = (language: string) => datePlaceholder(dateFormatter(language).formatToParts(example), dictionaries[`../../locales/${language}.json`].ui.dateLetters);

    it('spells the date pattern in the letters of each language', () => {
        expect(placeholder('it')).toBe('GG/MM/AAAA');
        expect(placeholder('en')).toBe('DD/MM/YYYY');
        expect(placeholder('de')).toBe('TT.MM.JJJJ');
        expect(placeholder('fr')).toBe('JJ/MM/AAAA');
        expect(placeholder('zh')).toBe('年/月/日');
    });

    it('names day, month and year in every language', () => {
        for (const language of Object.keys(languages)) {
            expect(dictionaries[`../../locales/${language}.json`].ui.dateLetters.split(' '), language).toHaveLength(3);
            expect(placeholder(language), language).not.toMatch(/\d/);
        }
    });
});

describe('firstWeekday', () => {
    it('starts English weeks on Monday, like its British date format', () => {
        expect(firstWeekday('en-US')).toBe(0);
        expect(firstWeekday(intlLocale('en'))).toBe(1);
        expect(firstWeekday(intlLocale('it'))).toBe(1);
    });
});

describe('readDate', () => {
    it('reads back what the formatter prints in every language', () => {
        for (const language of Object.keys(languages)) {
            const formatter = dateFormatter(language);
            for (const iso of ['2031-04-27', '2024-02-29', '2030-12-01']) {
                const printed = formatter.format(new Date(`${iso}T12:00:00Z`));
                expect(readDate(printed, orderFor(language)), `${language}: ${printed}`).toBe(iso);
            }
        }
    });

    it('accepts the separators used by the supported locales', () => {
        expect(readDate('27/04/2031', dmy)).toBe('2031-04-27');
        expect(readDate('27.04.2031', dmy)).toBe('2031-04-27');
        expect(readDate('27-04-2031', dmy)).toBe('2031-04-27');
        expect(readDate('27 04 2031', dmy)).toBe('2031-04-27');
        expect(readDate('2031/04/27', ymd)).toBe('2031-04-27');
        expect(readDate('04/27/2031', mdy)).toBe('2031-04-27');
    });

    it('pads single-digit days and months', () => {
        expect(readDate('7/4/2031', dmy)).toBe('2031-04-07');
        expect(readDate('2031/4/7', ymd)).toBe('2031-04-07');
        expect(readDate('4/7/2031', mdy)).toBe('2031-04-07');
    });

    it('splits compact eight-digit entry by the locale order', () => {
        expect(readDate('27042031', dmy)).toBe('2031-04-27');
        expect(readDate('20310427', ymd)).toBe('2031-04-27');
        expect(readDate('04272031', mdy)).toBe('2031-04-27');
        expect(readDate('20310427', dmy)).toBe('');
    });

    it('always accepts ISO dates, whatever the locale', () => {
        expect(readDate('2031-04-27', dmy)).toBe('2031-04-27');
        expect(readDate('2031-04-27', ymd)).toBe('2031-04-27');
        expect(readDate('2031-04-27', mdy)).toBe('2031-04-27');
    });

    it('trims surrounding whitespace', () => {
        expect(readDate('  27/04/2031 \n', dmy)).toBe('2031-04-27');
    });

    it('reads Arabic-Indic, Persian, Bengali and Devanagari digits', () => {
        expect(readDate('٢٧/٠٤/٢٠٣١', dmy)).toBe('2031-04-27');
        expect(readDate('۲۷/۰۴/۲۰۳۱', dmy)).toBe('2031-04-27');
        expect(readDate('২৭/০৪/২০৩১', dmy)).toBe('2031-04-27');
        expect(readDate('२७/०४/२०३१', dmy)).toBe('2031-04-27');
        expect(readDate('٢٧٠٤٢٠٣١', dmy)).toBe('2031-04-27');
    });

    it('drops bidirectional marks', () => {
        expect(readDate('‏27‏/04‎/2031؜', dmy)).toBe('2031-04-27');
    });

    it('trims whitespace hidden behind bidirectional marks', () => {
        expect(readDate('‏ 27/04/2031', dmy)).toBe('2031-04-27');
        expect(readDate('27/04/2031 ‎', dmy)).toBe('2031-04-27');
    });

    it('rejects days that do not exist', () => {
        expect(readDate('31/02/2031', dmy)).toBe('');
        expect(readDate('31/04/2031', dmy)).toBe('');
        expect(readDate('00/04/2031', dmy)).toBe('');
        expect(readDate('27/13/2031', dmy)).toBe('');
        expect(readDate('27/00/2031', dmy)).toBe('');
        expect(readDate('2031-02-31', dmy)).toBe('');
        expect(readDate('2031-13-01', dmy)).toBe('');
    });

    it('accepts 29 February only in leap years', () => {
        expect(readDate('29/02/2024', dmy)).toBe('2024-02-29');
        expect(readDate('29/02/2000', dmy)).toBe('2000-02-29');
        expect(readDate('29/02/2031', dmy)).toBe('');
        expect(readDate('29/02/1900', dmy)).toBe('');
        expect(readDate('29/02/2100', dmy)).toBe('');
    });

    it('rejects year zero but accepts the rest of the four-digit range', () => {
        expect(readDate('01/01/0000', dmy)).toBe('');
        expect(readDate('0000-01-01', dmy)).toBe('');
        expect(readDate('01/01/0001', dmy)).toBe('0001-01-01');
        expect(readDate('31/12/9999', dmy)).toBe('9999-12-31');
    });

    it('rejects malformed input', () => {
        for (const raw of ['', '   ', 'abc', '27/04', '27/04/2031/1', '27/04/31', '27/04/02031',
            '127/04/2031', '27/004/2031', '27//04/2031', '27/04/20a1', '2704203', '270420311',
            '27 / 04 / 2031', '+27/04/2031', '27,04,2031', '2031-4-27T00:00']) {
            expect(readDate(raw, dmy), JSON.stringify(raw)).toBe('');
        }
    });

    it('applies the year length rule to the year position of the locale', () => {
        expect(readDate('2031/04/27', dmy)).toBe('');
        expect(readDate('27/04/2031', ymd)).toBe('');
        expect(readDate('2031-4-27', ymd)).toBe('2031-04-27');
    });
});

describe('minuteOptions', () => {
    it('offers five-minute steps', () => {
        expect(minuteOptions()).toEqual(['00', '05', '10', '15', '20', '25', '30', '35', '40', '45', '50', '55']);
        expect(minuteOptions('30')).toHaveLength(12);
    });

    it('keeps a saved minute that is off the grid, in order', () => {
        expect(minuteOptions('07')).toEqual(['00', '05', '07', '10', '15', '20', '25', '30', '35', '40', '45', '50', '55']);
        expect(minuteOptions('59').at(-1)).toBe('59');
    });

    it('ignores values that are not minutes', () => {
        for (const value of ['', '7', '60', 'ab', '5x']) expect(minuteOptions(value), value).toHaveLength(12);
    });
});
