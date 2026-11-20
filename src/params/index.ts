import { defineParams } from '@sveltejs/kit/params';
import { token } from './token.ts';

/** Route param matchers, used as `[name=token]`. */
export const params = defineParams({ token });
