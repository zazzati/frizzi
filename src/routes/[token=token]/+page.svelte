<script lang="ts">
 import { afterNavigate } from '$app/navigation';
 import { page } from '$app/state';
 import EventShell from '#lib/components/EventShell.svelte';
 import RecoverLink from '#lib/components/RecoverLink.svelte';
 import VotePanel from '#lib/components/VotePanel.svelte';
 import { eventPage } from '#lib/event-page.ts';
 const view = eventPage('event');
 afterNavigate(() => void view.open(page.params.token!));
</script>

<EventShell {view} let:event>
 <VotePanel {view} {event}/>
 <RecoverLink {view}/>
</EventShell>
