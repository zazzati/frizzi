// Keeps the colour tokens in src/style.css at WCAG 2.2 AA contrast on the backgrounds they sit on.
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const css = readFileSync(new URL('../../src/style.css', import.meta.url), 'utf8');
const token = (name: string) => css.match(new RegExp(`--${name}:\\s*(#[0-9a-f]{6})`, 'i'))![1];
const luminance = (hex: string) => {
    const [r, g, b] = hex.slice(1).match(/../g)!.map(part => parseInt(part, 16) / 255)
        .map(value => value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4);
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
const contrast = (a: string, b: string) => {
    const [light, dark] = [luminance(a), luminance(b)].sort((x, y) => y - x);
    return (light + 0.05) / (dark + 0.05);
};
// White panels, the page, inputs, chosen dates, the message composer and notices.
const backgrounds = ['#ffffff', '#f7f9f5', '#fbfcfa', '#f1f9e9', '#f4f8ee', '#e9f5dc'];

describe('colour tokens', () => {
    it.each([['muted', 4.5], ['control-border', 3], ['accent', 3]] as const)('--%s reaches %s:1', (name, minimum) => {
        for (const background of backgrounds) expect(contrast(token(name), background), background).toBeGreaterThanOrEqual(minimum);
    });

    it('--proposed-border separates proposed days from the calendar and their fill', () => {
        for (const background of ['#fbfcfa', '#e8f2dc']) expect(contrast(token('proposed-border'), background)).toBeGreaterThanOrEqual(3);
    });
});
