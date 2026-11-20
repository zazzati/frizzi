<script lang="ts">
 import { afterNavigate } from '$app/navigation';
 import { page } from '$app/state';
 import EventShell from '#lib/components/EventShell.svelte';
 import RecoverLink from '#lib/components/RecoverLink.svelte';
 import VotePanel from '#lib/components/VotePanel.svelte';
 import { eventPage } from '#lib/event-page.ts';
 // A participant's private link: the saved answer loads with the event and saving replaces it.
 const view = eventPage('edit');
 afterNavigate(() => void view.open(page.params.token!, page.params.secret!));
</script>

<EventShell {view} let:event>
 <VotePanel {view} {event}/>
 <RecoverLink {view}/>
</EventShell>
