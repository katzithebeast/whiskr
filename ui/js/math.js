// Math in answers: $…$, $$…$$, \(…\), \[…\] → KaTeX (bundled in ui/vendor/katex).
// Used by the Markdown renderers: pull the formulas out first (so Markdown
// doesn't mangle * and _ inside them), render the rest, then put them back.

const OPEN = "\u0001", CLOSE = "\u0002";

/** Replace formulas with placeholders. Unfinished ones (still streaming) stay as text. */
export function extractMath(src) {
  const blocks = [];
  const put = (tex, display) => {
    blocks.push({ tex: tex.trim(), display });
    return `${OPEN}${blocks.length - 1}${CLOSE}`;
  };
  let s = String(src || "");
  // display: $$ … $$  and  \[ … \]  (may span lines)
  s = s.replace(/\$\$([\s\S]+?)\$\$/g, (_, t) => put(t, true));
  s = s.replace(/\\\[([\s\S]+?)\\\]/g, (_, t) => put(t, true));
  // inline: \( … \)
  s = s.replace(/\\\(([\s\S]+?)\\\)/g, (_, t) => put(t, false));
  // inline: $ … $ — not prices: no space right inside, no digit right after the closing $
  s = s.replace(/(^|[^\\$\w])\$(?!\s)([^$\n]+?)(?<!\s)\$(?![\d$\w])/g, (_, pre, t) => pre + put(t, false));
  return { text: s, blocks };
}

function renderOne(b) {
  const k = window.katex;
  if (!k) return b.display ? `<div class="math-raw">${escapeHtml(b.tex)}</div>` : `<code>${escapeHtml(b.tex)}</code>`;
  try {
    return k.renderToString(b.tex, { displayMode: b.display, throwOnError: false, output: "html", strict: "ignore" });
  } catch {
    return `<code>${escapeHtml(b.tex)}</code>`;
  }
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
}

/** Put rendered formulas back into the finished HTML. */
export function restoreMath(html, blocks) {
  if (!blocks.length) return html;
  return html
    // a display formula alone in a paragraph: no <p> around the block
    .replace(new RegExp(`<p>${OPEN}(\\d+)${CLOSE}</p>`, "g"), (m, i) => (blocks[i]?.display ? renderOne(blocks[i]) : `<p>${renderOne(blocks[i])}</p>`))
    .replace(new RegExp(`${OPEN}(\\d+)${CLOSE}`, "g"), (_, i) => (blocks[i] ? renderOne(blocks[i]) : ""));
}
