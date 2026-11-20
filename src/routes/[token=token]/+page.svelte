<script lang="ts">
 import { afterNavigate } from '$app/navigation';
 import { page } from '$app/state';
 import EventShell from '#lib/components/EventShell.svelte';
 import RecoverLink from '#lib/components/RecoverLink.svelte';
 import VotePanel from '#lib/components/VotePanel.svelte';
 import { EventPage } from '#lib/event.svelte.ts';
 const view = new EventPage('event');
 afterNavigate(() => void view.open(page.params.token!));
</script>

<EventShell {view}>
 {#snippet children(event)}
  <VotePanel {view} {event}/>
  <RecoverLink {view}/>
 {/snippet}
</EventShell>
