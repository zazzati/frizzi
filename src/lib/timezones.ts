import { dateTimeFormat } from './intl.ts';

/** The IANA id with the casing the API expects (`europe/rome` → `Europe/Rome`), or `''` when the runtime does not know the zone. */
export function canonicalZone(zone: string) {
    const trimmed = zone.trim();
    if (!trimmed) return '';
    try {
        const resolved = dateTimeFormat('en', { timeZone: trimmed }).resolvedOptions().timeZone;
        return resolved.toLowerCase() === trimmed.toLowerCase() ? resolved : trimmed;
    } catch { return ''; }
}

/** The browser's zone, or Europe/Rome when the runtime cannot tell. */
export function viewerZone() {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || 'Europe/Rome';
}

function zonePart(zone: string, locale: string, timeZoneName: 'longGeneric' | 'shortOffset', at: Date) {
    try {
        return dateTimeFormat(locale, { timeZone: zone, timeZoneName }).formatToParts(at).find(part => part.type === 'timeZoneName')?.value || '';
    } catch { return ''; }
}

/** A readable zone name in the page language, e.g. "Ora dell’Europa centrale"; the id when there is none. */
export function zoneName(zone: string, locale: string, at = new Date()) {
    return zonePart(zone, locale, 'longGeneric', at) || zone;
}

/** "Central European Time (GMT+1)", or `''` when the runtime has neither a name nor an offset. */
export function zoneDetails(zone: string, locale: string, at = new Date()) {
    const name = zonePart(zone, locale, 'longGeneric', at), offset = zonePart(zone, locale, 'shortOffset', at);
    return [name !== zone ? name : '', offset && `(${offset})`].filter(Boolean).join(' ');
}

/** "Europe/Rome — Central European Time (GMT+1)": the id stays first so typing it still matches. */
export function zoneLabel(zone: string, locale: string, at = new Date()) {
    const details = zoneDetails(zone, locale, at);
    return details ? `${zone} — ${details}` : zone;
}

/** Every zone the runtime knows, plus the current value when it is an alias missing from that list. */
export function zoneOptions(locale: string, current = '', at = new Date()) {
    const zones: string[] = typeof Intl.supportedValuesOf === 'function' ? Intl.supportedValuesOf('timeZone') : [];
    const all = current && !zones.includes(current) && canonicalZone(current) ? [...zones, current].sort() : zones;
    return all.map(zone => ({ zone, label: zoneLabel(zone, locale, at) }));
}

/** Milliseconds the zone is ahead of UTC at the instant. */
function offsetAt(instant: number, zone: string) {
    const parts = Object.fromEntries(dateTimeFormat('en-US', {
        timeZone: zone, hourCycle: 'h23', year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit'
    }).formatToParts(instant).map(part => [part.type, part.value]));
    return Date.UTC(+parts.year, +parts.month - 1, +parts.day, +parts.hour, +parts.minute, +parts.second) - Math.floor(instant / 1000) * 1000;
}

/**
 * The same moment as a date and `HH:MM` time on the wall clock of another zone,
 * e.g. 2030-02-03 20:00 in Europe/Rome is 2030-02-03 14:00 in America/New_York.
 */
export function convertTime(date: string, time: string, from: string, to: string) {
    const wall = Date.parse(`${date}T${time.slice(0, 5)}:00Z`);
    if (!Number.isFinite(wall)) return null;
    try {
        let instant = wall - offsetAt(wall, from);
        instant = wall - offsetAt(instant, from);
        const shifted = new Date(instant + offsetAt(instant, to)).toISOString();
        return { date: shifted.slice(0, 10), time: shifted.slice(11, 16) };
    } catch { return null; }
}
