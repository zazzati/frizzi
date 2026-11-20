export type Route =
    | { mode: 'create'; publicToken: ''; secret: '' }
    | { mode: 'event' | 'edit' | 'admin'; publicToken: string; secret: string };

const token = /^[A-Za-z0-9_-]{21}$/;

/**
 * Reads the catch-all path: nothing creates an event, `<public>` votes, `<public>/<edit>`
 * changes a response and `<public>/admin/<secret>` manages the event. Anything else is null.
 */
export function parsePath(path: string | undefined): Route | null {
    const parts = path?.split('/').filter(Boolean) || [];
    const [publicToken, second, third] = parts;
    if (!parts.length) return { mode: 'create', publicToken: '', secret: '' };
    if (!token.test(publicToken) || parts.length > 3) return null;
    if (parts.length === 1) return { mode: 'event', publicToken, secret: '' };
    if (parts.length === 2) return second !== 'admin' && token.test(second) ? { mode: 'edit', publicToken, secret: second } : null;
    return second === 'admin' && token.test(third) ? { mode: 'admin', publicToken, secret: third } : null;
}
