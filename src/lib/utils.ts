import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';
export function cn(...inputs: ClassValue[]) { return twMerge(clsx(inputs)); }
/** A submit handler that keeps the browser from navigating, for forms the page sends itself. */
export function onSubmit(handler: () => unknown) {
 return (event: SubmitEvent) => { event.preventDefault(); void handler(); };
}
