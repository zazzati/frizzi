import { addMessages, init, locale, format, register } from 'svelte-i18n';
import { derived } from 'svelte/store';
import { dateTimeFormat, numberFormat } from './intl.ts';
import it from '../../locales/it.json';
export const languages: Record<string,string> = { it:'Italiano', en:'English', es:'Español', zh:'中文', hi:'हिन्दी', ar:'العربية', fr:'Français', bn:'বাংলা', pt:'Português', ru:'Русский', de:'Deutsch' };
// Italian is bundled for server rendering and as the fallback; other languages load when chosen.
addMessages('it', it);
const dictionaries = import.meta.glob<{ default: typeof it }>(['../../locales/*.json', '!../../locales/it.json']);
for (const [path, load] of Object.entries(dictionaries)) register(path.slice(path.lastIndexOf('/') + 1, -5), load);
init({ fallbackLocale:'it', initialLocale:'it' });
export const lang = locale;
/** The saved language when supported, else the first supported browser language, else Italian. */
export function detectLocale(saved: string | null | undefined, preferred: readonly string[] = []) {
    if (saved && Object.hasOwn(languages, saved)) return saved;
    const match = preferred.map(tag => tag.toLowerCase().split(/[-_]/)[0]).find(code => Object.hasOwn(languages, code));
    return match ?? 'it';
}
/** ICU plural messages: format them with $plural(key, count), not $t. */
export const pluralKeys = ['availableCount', 'availableOf', 'messageQueued', 'messageConfirmText'] as const;
export type PluralKey = typeof pluralKeys[number];
export type Messages = Omit<typeof it.ui, PluralKey>;
const plain = Object.keys(it.ui).filter(key => !(pluralKeys as readonly string[]).includes(key));
export const t = derived(format, $format => Object.fromEntries(plain.map(key=>[key,$format(`ui.${key}`)])) as Messages);
/**
 * `count` picks the plural form of the active language. Messages print `{n}` and the other
 * values already formatted, so their digits match the rest of the page (ICU `#` would not).
 */
export const plural = derived([format, locale], ([$format, $locale]) => {
    const numbers = numberFormat(intlLocale($locale));
    return (key: PluralKey, count: number, values: Record<string, number> = {}) => $format(`ui.${key}`, { values: {
        ...Object.fromEntries(Object.entries(values).map(([name, value]) => [name, numbers.format(value)])),
        count, n: numbers.format(count)
    } });
});
export function errorText(code: string, messages: Messages): string {
    const keys: Record<string,keyof Messages> = { invalid_input:'invalidInput', email_exists:'emailExists', not_found:'notFound', multiple_votes_exist:'multipleVotesExist', rate_limited:'rateLimited', event_closed:'eventClosed', invalid_deadline:'invalidDeadline', invalid_dates:'invalidDates', deadline_passed:'deadlinePassed', tiebreak_unavailable:'tiebreakUnavailable', service_unavailable:'serviceUnavailable', copy_failed:'copyFailed' };
    const [kind, reference] = code.split(':', 2);
    const message = messages[Object.hasOwn(keys, kind) ? keys[kind] : 'error'];
    return reference && /^[a-f0-9-]{36}$/.test(reference) ? `${message} ${messages.supportCode} ${reference}` : message;
}
const dateLocales: Record<string, string> = {
    it: 'it-IT', en: 'en-GB', es: 'es-ES', zh: 'zh-CN', hi: 'hi-IN', ar: 'ar-SA',
    fr: 'fr-FR', bn: 'bn-BD', pt: 'pt-PT', ru: 'ru-RU', de: 'de-DE'
};
/** The regional locale used for every Intl formatter, so dates, times, weeks and numbers agree. */
export function intlLocale(language: string | null | undefined) {
    return Object.hasOwn(dateLocales, language ?? '') ? dateLocales[language!] : dateLocales.it;
}
export function dateFormatter(language: string | null | undefined) {
    return dateTimeFormat(intlLocale(language), {
        day: '2-digit', month: '2-digit', year: 'numeric', timeZone: 'UTC', calendar: 'gregory'
    });
}
/** An event date for reading, with the month spelled out ("sab 3 feb 2030"); typed dates keep the numeric dateFormatter. */
export function formatDate(date: string, language: string | null | undefined) {
    return dateTimeFormat(intlLocale(language), {
        // Portuguese abbreviated months fall back to a numeric pattern in CLDR, so it spells them out.
        weekday: 'short', day: 'numeric', month: intlLocale(language) === 'pt-PT' ? 'long' : 'short', year: 'numeric', timeZone: 'UTC', calendar: 'gregory'
    }).format(new Date(`${date}T12:00:00Z`));
}
export function formatTime(time: string, locale: string | null | undefined) {
    return dateTimeFormat(intlLocale(locale),{hour:'numeric',minute:'2-digit',timeZone:'UTC'}).format(new Date(`2000-01-01T${time}Z`));
}
