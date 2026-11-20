import { describe, expect, it, vi } from 'vitest';
import { cn, onSubmit } from './utils.ts';

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

describe('onSubmit', () => {
    it('prevents the browser submission and runs the handler', () => {
        const handler = vi.fn(() => Promise.resolve());
        const event = { preventDefault: vi.fn() } as unknown as SubmitEvent;
        onSubmit(handler)(event);
        expect(event.preventDefault).toHaveBeenCalledOnce();
        expect(handler).toHaveBeenCalledOnce();
    });
});
