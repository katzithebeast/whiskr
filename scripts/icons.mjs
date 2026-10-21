// Renders the Whiskr app icon (black rounded square + cream cat) to a 1024 PNG.
// `npx tauri icon` then generates every platform size from it.
import { Resvg } from "@resvg/resvg-js";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";

const cat = readFileSync(new URL("../ui/img/cat.svg", import.meta.url), "utf8");
const inner = cat.replace(/^[\s\S]*?<svg[^>]*>/, "").replace(/<\/svg>\s*$/, "");

// Recolour: cat becomes cream, face features black.
const recoloured = inner
  .replace(/stroke="#111111"/g, 'stroke="#F8F6EF"')
  .replace(/fill="#111111"/g, 'fill="#F8F6EF"')
  .replace(/stroke="#F8F6EF" stroke-width="5"/, 'stroke="#111111" stroke-width="5"');

const appIcon = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">
  <rect x="64" y="64" width="896" height="896" rx="210" fill="#111111"/>
  <g transform="translate(152 210) scale(4.5)">${recoloured}</g>
</svg>`;

mkdirSync(new URL("../build/", import.meta.url), { recursive: true });
writeFileSync(new URL("../build/app-icon.svg", import.meta.url), appIcon);
const png = new Resvg(appIcon, { fitTo: { mode: "width", value: 1024 } }).render().asPng();
writeFileSync(new URL("../build/app-icon.png", import.meta.url), png);

// Preview of the plain cat logo on cream, for visual checks.
const logo = new Resvg(cat, { fitTo: { mode: "width", value: 640 }, background: "#F8F6EF" }).render().asPng();
writeFileSync(new URL("../build/cat-preview.png", import.meta.url), logo);
console.log("icons rendered");
