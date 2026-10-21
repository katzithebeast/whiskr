// Tiny, safe Markdown → HTML (headings, lists, bold/italic, code, links).
const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));

function inline(s) {
  return s
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<b>$1</b>")
    .replace(/(^|[^*])\*([^*\n]+)\*/g, "$1<i>$2</i>")
    .replace(/\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g, '<a href="$2" data-url="$2">$1</a>')
    .replace(/(^|[\s(])(https?:\/\/[^\s<)]+)/g, '$1<a href="$2" data-url="$2">$2</a>');
}

export function md(src) {
  const out = [];
  String(src || "")
    .split(/```/)
    .forEach((part, i) => {
      if (i % 2 === 1) {
        const nl = part.indexOf("\n");
        out.push(`<pre><code>${esc((nl >= 0 ? part.slice(nl + 1) : part).replace(/\n$/, ""))}</code></pre>`);
        return;
      }
      let list = null;
      const close = () => {
        if (list) out.push(`</${list}>`);
        list = null;
      };
      for (const raw of part.split("\n")) {
        const line = esc(raw);
        let m;
        if ((m = line.match(/^\s*[-*•]\s+(.*)$/))) {
          if (list !== "ul") (close(), out.push("<ul>"), (list = "ul"));
          out.push(`<li>${inline(m[1])}</li>`);
        } else if ((m = line.match(/^\s*\d+[.)]\s+(.*)$/))) {
          if (list !== "ol") (close(), out.push("<ol>"), (list = "ol"));
          out.push(`<li>${inline(m[1])}</li>`);
        } else if ((m = line.match(/^#{1,4}\s+(.*)$/))) {
          close();
          out.push(`<h4>${inline(m[1])}</h4>`);
        } else if (/^\s*(-{3,}|\*{3,})\s*$/.test(raw)) {
          close();
        } else if (line.trim()) {
          close();
          out.push(`<p>${inline(line)}</p>`);
        } else close();
      }
      close();
    });
  return out.join("");
}
