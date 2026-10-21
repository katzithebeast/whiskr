// The Whiskr cat, as an inline SVG so it can be recoloured with CSS variables.
export function cat({ body = "currentColor", eyes = "var(--cream)", cls = "" } = {}) {
  return `<svg class="cat ${cls}" viewBox="0 0 160 120" aria-hidden="true">
    <g class="cat-whiskers" fill="none" stroke="${body}" stroke-width="4.2" stroke-linecap="round">
      <path d="M44 74 Q24 70 4 72"/><path d="M44 81 Q24 82 6 88"/><path d="M46 88 Q30 93 14 102"/>
      <path d="M116 74 Q136 70 156 72"/><path d="M116 81 Q136 82 154 88"/><path d="M114 88 Q130 93 146 102"/>
    </g>
    <path class="cat-head" fill="${body}" d="M35 46 C33 34 32 20 34 10 C35 5 39 4 43 7 C51 13 58 20 63 26 C74 23 86 23 97 26 C102 20 109 13 117 7 C121 4 125 5 126 10 C128 20 127 34 125 46 C132 56 133 70 128 82 C120 102 100 110 80 110 C60 110 40 102 32 82 C27 70 28 56 35 46 Z"/>
    <g class="cat-eyes" fill="none" stroke="${eyes}" stroke-width="5" stroke-linecap="round">
      <path d="M52 66 Q60 57 68 66"/><path d="M92 66 Q100 57 108 66"/>
    </g>
  </svg>`;
}

// Pretty key names for a shortcut spec like "ctrl+shift+Space".
const NAMES = {
  ctrl: "Ctrl", control: "Ctrl", shift: "Shift", alt: "Alt", option: "Alt",
  super: "Win", cmd: "Win", command: "Win",
  space: "Space", backquote: "`", minus: "-", equal: "=", comma: ",", period: ".",
  slash: "/", backslash: "\\", semicolon: ";", quote: "'", bracketleft: "[", bracketright: "]",
  enter: "Enter", tab: "Tab", backspace: "⌫", arrowup: "↑", arrowdown: "↓",
  arrowleft: "←", arrowright: "→", insert: "Ins", delete: "Del", home: "Home", end: "End",
  pageup: "PgUp", pagedown: "PgDn", capslock: "Caps",
};

export function keyLabels(spec = "") {
  return spec.split("+").filter(Boolean).map((raw) => {
    const k = raw.trim();
    const low = k.toLowerCase();
    if (NAMES[low]) return NAMES[low];
    if (/^key[a-z]$/i.test(k)) return k.slice(3).toUpperCase();
    if (/^digit\d$/i.test(k)) return k.slice(5);
    if (/^numpad/i.test(k)) return "Num " + k.slice(6);
    return k.length === 1 ? k.toUpperCase() : k;
  });
}

export function keycaps(spec) {
  return keyLabels(spec).map((k) => `<kbd>${k}</kbd>`).join('<span class="plus">+</span>');
}
