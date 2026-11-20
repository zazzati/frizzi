<script lang="ts">
 import { Copy, KeyRound } from '@lucide/svelte';
 import { Button } from '#lib/components/ui/button/index.ts';
 import { t } from '#lib/i18n.ts';
 /** The organizer's private link. */
 export let link: string;
 export let copied = false;
 export let copy: () => void;
</script>

<section class="admin-link" aria-labelledby="admin-link-title">
 <h3 id="admin-link-title"><KeyRound size={16} aria-hidden="true"/>{$t.adminLinkTitle}</h3>
 <p id="admin-link-note">{$t.adminLinkNote}</p>
 <div class="copy-row">
  <input class="grow" dir="ltr" readonly value={link} aria-labelledby="admin-link-title" aria-describedby="admin-link-note" on:focus={event=>event.currentTarget.select()}/>
  <Button variant="outline" onclick={copy}><Copy size={16} aria-hidden="true"/>{copied?$t.copied:$t.copy}</Button>
 </div>
</section>
