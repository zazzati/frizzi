<script lang="ts">
 import { Copy, KeyRound } from '@lucide/svelte';
 import { Button } from '#lib/components/ui/button/index.ts';
 import { t } from '#lib/i18n.ts';
 /** `link` is the organizer's private link. */
 let { link, copied = false, copy }: { link: string; copied?: boolean; copy: () => void } = $props();
</script>

<section class="admin-link" aria-labelledby="admin-link-title">
 <h3 id="admin-link-title"><KeyRound size={16} aria-hidden="true"/>{$t.adminLinkTitle}</h3>
 <p id="admin-link-note">{$t.adminLinkNote}</p>
 <div class="copy-row">
  <input class="grow" dir="ltr" readonly value={link} aria-labelledby="admin-link-title" aria-describedby="admin-link-note" onfocus={event=>event.currentTarget.select()}/>
  <Button variant="outline" onclick={copy}><Copy size={16} aria-hidden="true"/>{copied?$t.copied:$t.copy}</Button>
 </div>
</section>
