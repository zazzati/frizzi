import { describe, expect, it } from 'vitest';
import { cn } from './utils.ts';

describe('cn', () => {
    it('joins conditional class names', () => {
        expect(cn('a', false && 'b', null, undefined, ['c', { d: true, e: false }])).toBe('a c d');
    });

    it('lets later Tailwind utilities win over conflicting ones', () => {
        expect(cn('px-2 py-1 bg-red-500', 'px-4', 'bg-blue-500')).toBe('py-1 px-4 bg-blue-500');
    });

    it('keeps utilities that do not conflict', () => {
        expect(cn('text-sm font-bold', 'text-red-500')).toBe('text-sm font-bold text-red-500');
    });

    it('returns an empty string without classes', () => {
        expect(cn()).toBe('');
    });
});
