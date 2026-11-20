// Intl formatters are slow to build and immutable, so each locale and option set is built once.
const dates = new Map<string, Intl.DateTimeFormat>();
const numbers = new Map<string, Intl.NumberFormat>();

function cached<T>(cache: Map<string, T>, locale: string, options: object, build: () => T) {
    const key = `${locale} ${JSON.stringify(options)}`;
    let formatter = cache.get(key);
    // A constructor that throws (e.g. an unknown time zone) caches nothing and throws again next time.
    if (!formatter) cache.set(key, formatter = build());
    return formatter;
}

export function dateTimeFormat(locale: string, options: Intl.DateTimeFormatOptions = {}) {
    return cached(dates, locale, options, () => new Intl.DateTimeFormat(locale, options));
}

export function numberFormat(locale: string, options: Intl.NumberFormatOptions = {}) {
    return cached(numbers, locale, options, () => new Intl.NumberFormat(locale, options));
}
