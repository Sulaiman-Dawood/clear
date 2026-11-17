export function esc(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

export function $(selector: string, root: ParentNode = document): HTMLElement {
  const el = root.querySelector<HTMLElement>(selector);
  if (!el) throw new Error(`Missing element: ${selector}`);
  return el;
}

/** Block browser shortcuts that make no sense in a desktop app (reload, print, find). */
export function lockDownWebview(): void {
  window.addEventListener("keydown", (e) => {
    const key = e.key.toLowerCase();
    const ctrl = e.ctrlKey || e.metaKey;
    if (key === "f5" || (ctrl && ["r", "p", "f", "g", "j", "u"].includes(key))) {
      e.preventDefault();
    }
  });
  window.addEventListener("contextmenu", (e) => {
    const t = e.target as HTMLElement | null;
    if (!t?.closest("input, textarea")) e.preventDefault();
  });
}

const KEY_LABELS: Record<string, string> = {
  Super: "Win",
  PageUp: "Page Up",
  PageDown: "Page Down",
  PrintScreen: "Print Screen",
  ScrollLock: "Scroll Lock",
  ContextMenu: "Menu",
  Escape: "Esc",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
  NumpadAdd: "Num +",
  NumpadSubtract: "Num -",
  NumpadMultiply: "Num *",
  NumpadDivide: "Num /",
  NumpadDecimal: "Num .",
};

/** How a shortcut token reads to people, e.g. "PageDown" -> "Page Down". */
export function keyLabel(token: string): string {
  const num = token.match(/^Numpad(\d)$/);
  return KEY_LABELS[token] ?? (num ? `Num ${num[1]}` : token);
}

export function formatAccelerator(accel: string): string {
  return accel
    .split("+")
    .filter(Boolean)
    .map((part) => `<span class="kbd">${esc(keyLabel(part))}</span>`)
    .join("");
}
