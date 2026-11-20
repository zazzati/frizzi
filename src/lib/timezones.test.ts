import { describe, expect, it } from 'vitest';
import { canonicalZone, convertTime, zoneDetails, zoneLabel, zoneName, zoneOptions } from './timezones.ts';

const winter = new Date('2030-02-03T12:00:00Z');

describe('canonicalZone', () => {
    it('fixes the casing of known zones and rejects unknown ones', () => {
        expect(canonicalZone('Europe/Rome')).toBe('Europe/Rome');
        expect(canonicalZone(' europe/rome ')).toBe('Europe/Rome');
        expect(canonicalZone('UTC')).toBe('UTC');
        for (const zone of ['', 'invalid/timezone', 'Europe/Atlantis', 'Rome']) expect(canonicalZone(zone), zone).toBe('');
    });
});

describe('zone labels', () => {
    it('puts the id first, then the readable name and offset in the page language', () => {
        expect(zoneLabel('Europe/Rome', 'en-GB', winter)).toBe('Europe/Rome — Central European Time (GMT+1)');
        expect(zoneLabel('America/New_York', 'it-IT', winter)).toBe('America/New_York — Ora orientale USA (GMT-5)');
        expect(zoneDetails('Asia/Kolkata', 'en-GB', winter)).toBe('India Standard Time (GMT+5:30)');
    });

    it('follows daylight saving time at the given date', () => {
        expect(zoneDetails('Europe/Rome', 'en-GB', new Date('2030-07-01T12:00:00Z'))).toBe('Central European Time (GMT+2)');
    });

    it('names a zone for display and falls back to the id', () => {
        expect(zoneName('Europe/Rome', 'it-IT', winter)).toBe('Ora dell’Europa centrale');
        expect(zoneName('invalid/timezone', 'it-IT', winter)).toBe('invalid/timezone');
    });

    it('lists every runtime zone once, with the current alias when it is missing', () => {
        const options = zoneOptions('en-GB', 'Europe/Rome', winter);
        expect(options.length).toBeGreaterThan(300);
        expect(new Set(options.map(option => option.zone)).size).toBe(options.length);
        expect(options.find(option => option.zone === 'Europe/Rome')?.label).toBe('Europe/Rome — Central European Time (GMT+1)');
        expect(zoneOptions('en-GB', 'US/Pacific', winter).some(option => option.zone === 'US/Pacific')).toBe(true);
        expect(zoneOptions('en-GB', 'invalid/timezone', winter)).toHaveLength(options.length);
    });
});

describe('convertTime', () => {
    it('shows the same moment on another wall clock', () => {
        expect(convertTime('2030-02-03', '20:00', 'Europe/Rome', 'America/New_York')).toEqual({ date: '2030-02-03', time: '14:00' });
        expect(convertTime('2030-02-03', '20:00:00', 'Europe/Rome', 'Asia/Tokyo')).toEqual({ date: '2030-02-04', time: '04:00' });
        expect(convertTime('2030-02-03', '20:00', 'Europe/Rome', 'Europe/Berlin')).toEqual({ date: '2030-02-03', time: '20:00' });
    });

    it('handles half-hour zones and daylight saving time', () => {
        expect(convertTime('2030-07-01', '09:00', 'Asia/Kolkata', 'Europe/London')).toEqual({ date: '2030-07-01', time: '04:30' });
        expect(convertTime('2030-03-31', '12:00', 'Europe/Rome', 'UTC')).toEqual({ date: '2030-03-31', time: '10:00' });
        expect(convertTime('2030-03-30', '12:00', 'Europe/Rome', 'UTC')).toEqual({ date: '2030-03-30', time: '11:00' });
    });

    it('returns null for unknown zones or malformed input', () => {
        expect(convertTime('2030-02-03', '20:00', 'invalid/timezone', 'UTC')).toBeNull();
        expect(convertTime('2030-02-30x', '20:00', 'Europe/Rome', 'UTC')).toBeNull();
    });
});
