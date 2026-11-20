const escapes: Record<string, string> = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
const lines = (text: string) => text.replace(/[&<>"']/g, char => escapes[char]).replace(/\n/g, '<br>');
const formatting = ['b', 'strong', 'i', 'em', 'u'];

/**
 * The participant message as the email will show it. Mirrors `MessageText::parse` in
 * src/mail/template.rs: only b, strong, i, em, u and br survive, closing tags must match
 * the innermost open one, everything else is escaped and open tags are closed at the end.
 * `plain` is empty when the API would reject the message as blank.
 */
export function messagePreview(input: string) {
    let plain = '', html = '', rest = input;
    const open: string[] = [];
    for (let at = rest.indexOf('<'); at >= 0; at = rest.indexOf('<')) {
        const text = rest.slice(0, at), tail = rest.slice(at + 1), close = tail.indexOf('>');
        plain += text; html += lines(text);
        if (close < 0) { rest = rest.slice(at); break; }
        const tag = tail.slice(0, close);
        if (formatting.includes(tag)) { open.push(tag); html += `<${tag}>`; }
        else if (tag.startsWith('/') && open.at(-1) === tag.slice(1)) { open.pop(); html += `<${tag}>`; }
        else if (tag === 'br' || tag === 'br/' || tag === 'br /') { plain += '\n'; html += '<br>'; }
        else { plain += `<${tag}>`; html += '&lt;' + lines(`${tag}>`); }
        rest = tail.slice(close + 1);
    }
    plain += rest; html += lines(rest);
    html += open.reverse().map(tag => `</${tag}>`).join('');
    return { plain: plain.trim() ? plain : '', html };
}
