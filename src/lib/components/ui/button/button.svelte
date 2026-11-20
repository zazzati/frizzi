<script lang="ts">
 import type { HTMLButtonAttributes } from 'svelte/elements';
 import { tv, type VariantProps } from 'tailwind-variants';
 import { cn } from '#lib/utils.ts';
 const variants = tv({ base:'button inline-flex items-center justify-center gap-2 focus-visible:outline-2 disabled:pointer-events-none disabled:opacity-50', variants:{ variant:{ default:'dark', outline:'btn-outline', ghost:'text-button' } }, defaultVariants:{variant:'default'} });
 type Props = HTMLButtonAttributes & VariantProps<typeof variants>;
 let { class:className, variant, type='button', children, ...rest }: Props = $props();
</script>
<button {type} class={cn(variants({variant}),className)} {...rest}>{@render children?.()}</button>
