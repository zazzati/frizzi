import { describe, expect, it } from 'vitest';
import { messagePreview } from './message.ts';

// Mirrors MessageText::parse in src/mail/template.rs, so the preview matches the email.
describe('messagePreview', () => {
    it('keeps the formatting the email allows', () => {
        expect(messagePreview('<b>Ci vediamo!</b>\n<i>A presto</i> <u>alle</u> <strong>8</strong><br><em>ok</em>')).toEqual({
            plain: 'Ci vediamo!\nA presto alle 8\nok',
            html: '<b>Ci vediamo!</b><br><i>A presto</i> <u>alle</u> <strong>8</strong><br><em>ok</em>'
        });
    });

    it('escapes every other tag and attribute', () => {
        expect(messagePreview('<img src=x onerror=alert(1)> & "q"').html).toBe('&lt;img src=x onerror=alert(1)&gt; &amp; &quot;q&quot;');
        expect(messagePreview('<b class="x">a</b>').html).toBe('&lt;b class=&quot;x&quot;&gt;a&lt;/b&gt;');
        expect(messagePreview('<script>alert(1)</script>').html).not.toMatch(/<script/);
    });

    it('closes open tags and escapes closing tags that do not match', () => {
        expect(messagePreview('<b><i>a</b>').html).toBe('<b><i>a&lt;/b&gt;</i></b>');
        expect(messagePreview('a</b>').html).toBe('a&lt;/b&gt;');
    });

    it('treats a lone angle bracket as text', () => {
        expect(messagePreview('1 < 2').html).toBe('1 &lt; 2');
        expect(messagePreview('a <b').html).toBe('a &lt;b');
    });

    it('is blank when only formatting or spaces are left, like the API check', () => {
        expect(messagePreview('<strong> </strong>').plain).toBe('');
        expect(messagePreview('<br>').plain).toBe('');
        expect(messagePreview('<x>').plain).toBe('<x>');
    });
});
