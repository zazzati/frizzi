<script lang="ts">
 let { source = null }: { source?: string | null } = $props();
</script>

<p class="mt-1 text-xs text-[#536357]">
 <a class="underline" href="https://www.geoapify.com/" target="_blank" rel="noopener">Geoapify</a>
 {#if source}{' · '}{source}{/if}
</p>
