<script lang="ts">
 import { Button } from '#lib/components/ui/button/index.ts';
 import { errorCode } from '#lib/api.ts';
 import { errorText, t } from '#lib/i18n.ts';
 import type { EventPage } from '#lib/event-page.ts';

 export let view: EventPage;
 // A vote rejected for a known address opens this form with that address.
 const { recoveryOpen, recoveryEmail } = view;
 let recovering = false, recovered = false, recoveryError = '';

 async function recover() {
   recoveryError = ''; recovered = false; recovering = true;
   try {
     await view.recover($recoveryEmail);
     recovered = true;
   } catch (error) {
     recoveryError = errorCode(error);
   } finally {
     recovering = false;
   }
 }
</script>

<details class="mt-5" bind:open={$recoveryOpen}>
 <summary class="cursor-pointer text-sm font-semibold">{$t.recoverLink}</summary>
 <form class="mt-2" on:submit|preventDefault={recover}>
  <label>{$t.email}<input name="recovery_email" type="email" required maxlength="255" autocomplete="email" bind:value={$recoveryEmail}/></label>
  <Button type="submit" variant="outline" class="mt-2" disabled={recovering}>{$t.recoverSend}</Button>
  {#if recovered}<p class="notice mt-2" role="status">{$t.recoverAccepted}</p>{/if}
  {#if recoveryError}<p class="error mt-2" role="alert">{errorText(recoveryError,$t)}</p>{/if}
 </form>
</details>
