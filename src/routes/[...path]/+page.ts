import { error } from '@sveltejs/kit';
import { parsePath } from '#lib/route.ts';
import type { PageLoad } from './$types';

// Malformed links get a real 404 and the error page; well-formed ones are checked by the API.
export const load: PageLoad = ({ params }) => {
 if (!parsePath(params.path)) error(404, 'not_found');
};
