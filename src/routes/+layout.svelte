<script lang="ts">
 import type { Snippet } from 'svelte';
 import { resolve } from '$app/paths';
 import '../style.css';
 // The same hashed file style.css points at: preloading it starts the download before the CSS is parsed.
 import interFont from '#lib/assets/InterVariable-LatinCyrillic.woff2?url';
 import { onMount } from 'svelte';
 import { track } from '#lib/telemetry.ts';
 import { Sparkles, Globe } from '@lucide/svelte';
 import { detectLocale, lang, languages, t } from '#lib/i18n.ts';
 let { children }: { children: Snippet } = $props();
 let ready = $state(false);
 onMount(()=>{
    let saved=null;
    try { saved=localStorage.getItem('locale'); } catch { /* Browser privacy mode can disable storage. */ }
    // Same choice as the inline script in app.html; the page is ready once that dictionary has loaded.
    void Promise.resolve(lang.set(detectLocale(saved,navigator.languages))).finally(()=>ready=true);
    let reported=false;
    const report=()=>{
      if(reported)return;
      reported=true;
      track('browser_runtime');
    };
    window.addEventListener('error',report);
    window.addEventListener('unhandledrejection',report);
    return ()=>{
      window.removeEventListener('error',report);
      window.removeEventListener('unhandledrejection',report);
    };
 });
 $effect(()=>{
    if (!ready) return;
    document.documentElement.lang=$lang || 'it';
    document.documentElement.dir=$lang==='ar'?'rtl':'ltr';
 });
 // Only an explicit choice is saved, so a first visit keeps following the browser language.
 function choose(code:string){
    lang.set(code);
    try { localStorage.setItem('locale',code); } catch { /* Keep the current language without persistence. */ }
 }
</script>
<svelte:head><title>Frizzi · {$t.tagline}</title><meta name="description" content={$t.intro}/><link rel="preload" href={interFont} as="font" type="font/woff2" crossorigin="anonymous"/></svelte:head>
<header data-ready={ready}>
 <a class="brand" href={resolve('')} aria-label="Frizzi"><span class="brand-icon"><Sparkles aria-hidden="true"/></span> frizzi<span class="brand-dot">.</span></a>
 <label class="language"><Globe size={18} aria-hidden="true"/><span class="sr-only">{$t.language}</span><select value={$lang} onchange={event=>choose(event.currentTarget.value)}>{#each Object.entries(languages) as [code,name]}<option value={code} lang={code}>{name}</option>{/each}</select></label>
</header>
<main>{@render children()}</main><footer>frizzi<span class="brand-dot">.</span><span class="footer-tagline">{$t.tagline}</span><a class="privacy-link" href={resolve('/privacy')}>{$t.privacyTitle}</a></footer>
