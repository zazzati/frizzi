export const fieldSizes = { day: 2, month: 2, year: 4 };
export type DatePart = keyof typeof fieldSizes;

/** Day, month and year in the order a formatter prints them. */
export function dateOrder(parts: Intl.DateTimeFormatPart[]): DatePart[] {
    return parts.filter((part): part is Intl.DateTimeFormatPart & {type: DatePart} =>
        part.type === 'day' || part.type === 'month' || part.type === 'year').map(part => part.type);
}

/** The formatter's date pattern spelled with the language's letters for day, month and year, e.g. `GG MM AAAA`. */
export function datePlaceholder(parts: Intl.DateTimeFormatPart[], letters: string) {
    const [day, month, year] = letters.split(' ');
    const names: Record<string, string> = { day, month, year };
    return parts.map(part => names[part.type] ?? part.value).join('');
}

/** First day of the week for a regional locale, 0 for Sunday to 6 for Saturday; Monday when the runtime cannot tell. */
export function firstWeekday(locale: string) {
    const local = new Intl.Locale(locale) as Intl.Locale & {
        getWeekInfo?: () => { firstDay: number };
        weekInfo?: { firstDay: number };
    };
    return (local.getWeekInfo?.().firstDay ?? local.weekInfo?.firstDay ?? 1) % 7;
}

/**
 * Reads a typed date in the field order of the user's locale and returns it as
 * `YYYY-MM-DD`, or `''` when it is not a real calendar date. ISO dates are always
 * accepted, eight digits without separators are split by field size, and native
 * digits (Arabic-Indic, Persian, Bengali, Devanagari) are read as ASCII.
 */
export function readDate(raw: string, order: DatePart[]): string {
    const normalized = raw.replace(/[‎‏؜]/g, '').trim().replace(/[٠-٩۰-۹০-৯०-९]/g, digit => {
        const start = ['٠','۰','০','०'].find(zero => digit >= zero && digit <= String.fromCharCode(zero.charCodeAt(0) + 9))!;
        return String(digit.charCodeAt(0) - start.charCodeAt(0));
    });
    let iso = normalized;
    if (!/^\d{4}-\d{2}-\d{2}$/.test(iso)) {
        let offset = 0;
        const parts = /^\d{8}$/.test(normalized)
            ? order.map(part => {
                const partValue = normalized.slice(offset, offset + fieldSizes[part]);
                offset += fieldSizes[part];
                return partValue;
            })
            : normalized.split(/[\/\.\s-]/);
        if (parts.length !== 3 || parts.some((part, i) =>
            !/^\d+$/.test(part) || (order[i] === 'year' ? part.length !== 4 : part.length > 2))) return '';
        const [year, month, day] = (['year','month','day'] as const).map(part =>
            parts[order.indexOf(part)].padStart(fieldSizes[part], '0'));
        iso = `${year}-${month}-${day}`;
    }
    if (iso.startsWith('0000-')) return '';
    const date = new Date(`${iso}T12:00:00Z`);
    return Number.isFinite(date.getTime()) && date.toISOString().slice(0, 10) === iso ? iso : '';
}

/** Minutes in five-minute steps, plus the saved minute when it is off that grid so older events keep their time. */
export function minuteOptions(current = '', step = 5) {
    const minutes = Array.from({ length: 60 / step }, (_, index) => String(index * step).padStart(2, '0'));
    return /^[0-5]\d$/.test(current) && !minutes.includes(current) ? [...minutes, current].sort() : minutes;
}
