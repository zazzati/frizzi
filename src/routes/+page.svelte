<script lang="ts">
 import { afterNavigate } from '$app/navigation';
 import CreateForm from '#lib/components/CreateForm.svelte';
 import CreateSuccess from '#lib/components/CreateSuccess.svelte';
 // The new event's links stay on this URL; following the home link again shows an empty form.
 let created: { link: string; title: string } | null = null;
 afterNavigate(() => created = null);
</script>

{#if created}
 <CreateSuccess link={created.link} title={created.title}/>
{:else}
 <CreateForm created={(link, title) => created = { link, title }}/>
{/if}
