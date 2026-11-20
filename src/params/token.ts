/**
 * Public, participant and organizer tokens: 21 URL-safe characters. Any other segment is not an
 * event link, so SvelteKit answers 404 with the error page instead of asking the API.
 */
export function token(param: string) {
 return /^[A-Za-z0-9_-]{21}$/.test(param) ? param : undefined;
}
