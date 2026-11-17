// Word-level diff (LCS over tokens) for the "Changes" view.
import { esc } from "./dom";

type Op = { type: "same" | "add" | "del"; text: string };

function tokenize(text: string): string[] {
  return text.match(/\s+|[\p{L}\p{N}_'’-]+|[^\s\p{L}\p{N}_]/gu) ?? [];
}

export function diffWords(before: string, after: string): Op[] | null {
  const a = tokenize(before);
  const b = tokenize(after);
  const n = a.length;
  const m = b.length;
  if (n * m > 4_000_000) return null; // too large to diff quickly

  // lengths[i][j] = LCS length of a[i:] and b[j:]
  const width = m + 1;
  const lengths = new Uint32Array((n + 1) * width);
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      lengths[i * width + j] =
        a[i] === b[j]
          ? lengths[(i + 1) * width + j + 1] + 1
          : Math.max(lengths[(i + 1) * width + j], lengths[i * width + j + 1]);
    }
  }

  const ops: Op[] = [];
  const push = (type: Op["type"], text: string) => {
    const last = ops[ops.length - 1];
    if (last && last.type === type) last.text += text;
    else ops.push({ type, text });
  };
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      push("same", a[i]);
      i++;
      j++;
    } else if (lengths[(i + 1) * width + j] >= lengths[i * width + j + 1]) {
      push("del", a[i++]);
    } else {
      push("add", b[j++]);
    }
  }
  while (i < n) push("del", a[i++]);
  while (j < m) push("add", b[j++]);
  return ops;
}

export function renderDiff(before: string, after: string): { html: string; changes: number } | null {
  const ops = diffWords(before, after);
  if (!ops) return null;
  // A run of deletions/insertions with nothing unchanged between them is one change.
  let changes = 0;
  let inRun = false;
  const html = ops
    .map((op) => {
      if (op.type === "same") {
        inRun = false;
        return esc(op.text);
      }
      if (/^\s+$/.test(op.text)) return op.type === "add" ? esc(op.text) : "";
      if (!inRun) changes++;
      inRun = true;
      return op.type === "add" ? `<ins>${esc(op.text)}</ins>` : `<del>${esc(op.text)}</del>`;
    })
    .join("");
  return { html, changes };
}
