// The popup: action menu, streaming result, and the small progress toast.
import "./lib/boot";
import "@fontsource-variable/geist";
import "./styles/popup.css";
import { listen } from "@tauri-apps/api/event";
import { api, errorMessage, type GenEvent, type Kind, type OpenPayload, type ToastPayload } from "./lib/ipc";
import { icon, logo } from "./lib/icons";
import { esc, lockDownWebview } from "./lib/dom";
import { LANGUAGES, languageCode } from "./lib/languages";
import { renderDiff } from "./lib/diff";
import { initTheme } from "./lib/theme";

lockDownWebview();
initTheme();

const root = document.getElementById("app")!;

type View = "menu" | "langs" | "result" | "toast";

interface Generation {
  id: number;
  label: string;
  kind: Kind;
  text: string;
  status: "streaming" | "done" | "error";
  error: string;
  tab: "result" | "changes";
  request: { action: string; instruction?: string; language?: string };
  /** Shown above the result, e.g. why it wasn't pasted. */
  note?: string;
}

let view: View = "menu";
let ctx: OpenPayload | null = null;
let gen: Generation | null = null;
let lastSize = { w: 0, h: 0 };

// ---------- sizing ----------

function measure(): { w: number; h: number } {
  const card = root.firstElementChild as HTMLElement | null;
  if (!card) return { w: 424, h: 260 };
  const r = card.getBoundingClientRect();
  return { w: Math.ceil(r.width) + 24, h: Math.ceil(r.height) + 24 };
}

function waitForPaint(): Promise<void> {
  return new Promise((resolve) => {
    const timer = window.setTimeout(resolve, 35);
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        window.clearTimeout(timer);
        resolve();
      });
    });
  });
}

async function present(id: number, focus: boolean, placement: "menu" | "toast" = "menu") {
  lastSize = measure();
  await waitForPaint();
  if (!root.firstElementChild) return;
  const size = measure();
  lastSize = size;
  await api.popupPresent(id, size.w, size.h, focus, placement);
  if (focus) focusDefault();
}

/** True while the user drags the output grip; the window is resized once at the end. */
let gripDragging = false;

/** The open animation plays once per opening, not on every redraw. */
let enterNext = false;
function enterClass(): string {
  const cls = enterNext ? " enter" : "";
  enterNext = false;
  return cls;
}

new ResizeObserver(() => {
  if (gripDragging || !root.firstElementChild) return;
  const size = measure();
  if (size.w !== lastSize.w || size.h !== lastSize.h) {
    lastSize = size;
    void api.popupResize(size.w, size.h);
  }
}).observe(root);

function focusDefault() {
  const target =
    root.querySelector<HTMLElement>("[data-autofocus]") ?? root.querySelector<HTMLElement>("input, textarea, button");
  target?.focus();
}

window.addEventListener("focus", () => {
  if (view === "menu" || view === "langs") focusDefault();
});

// ---------- menu ----------

function openMenu(payload: OpenPayload) {
  enterNext = true;
  ctx = payload;
  gen = null;
  view = "menu";
  resultHeight = DEFAULT_RESULT_HEIGHT;
  if (payload.ready) {
    // An in-place shortcut finished but couldn't paste safely: show its result.
    const r = payload.ready;
    gen = {
      id: 0,
      label: r.label,
      kind: r.kind,
      text: r.text,
      status: "done",
      error: "",
      tab: r.diffFirst && payload.hasSelection ? "changes" : "result",
      request: { action: r.action },
      note: r.note,
    };
    view = "result";
    renderResult();
  } else if (payload.autorun) {
    // Started from a shortcut (e.g. Translate): go straight to the result.
    void run(payload.autorun.action, undefined, payload.autorun.language ?? undefined);
  } else {
    renderMenu();
  }
  void present(payload.id, true);
}

function header(badge: string) {
  return `
    <div class="head">
      <div class="brand">${logo(18)}<span>Clear</span></div>
      <div class="spacer"></div>
      ${badge}
      <button class="icon-btn" data-open-settings title="Open Clear settings" aria-label="Open Clear settings">${icon("settings")}</button>
      <button class="icon-btn" data-close title="Close (Esc)">${icon("x")}</button>
    </div>`;
}

function selectionBadge(c: OpenPayload) {
  if (!c.hasSelection) return `<span class="badge badge-outline">No selection</span>`;
  const n = c.words;
  return `<span class="badge badge-soft">${n} ${n === 1 ? "word" : "words"} selected</span>`;
}

function renderMenu() {
  const c = ctx!;
  const blocked = !!c.problem;
  const notice = c.problem
    ? `<div class="notice error">${icon("alert")}<span>${esc(c.problem)}</span>
         <button class="btn btn-sm btn-outline" data-settings>Settings</button></div>`
    : !c.hasSelection
      ? `<div class="notice">${icon("info")}<span>Nothing is selected. Ask anything and insert the answer where you're typing, or select text first to use the actions.</span></div>`
      : "";

  const actionChips = c.actions
    .map(
      (a) =>
        `<button class="chip" data-action="${esc(a.id)}" title="${esc(a.description)}" ${blocked ? "disabled" : ""}>${icon(a.icon)}${esc(a.label)}</button>`,
    )
    .join("");
  const langChips =
    c.quickLanguages
      .map(
        (l) =>
          `<button class="chip lang" data-lang="${esc(l)}" title="Translate to ${esc(l)}" ${blocked ? "disabled" : ""}>${esc(languageCode(l))}</button>`,
      )
      .join("") +
    `<button class="chip more" data-more title="More languages" ${blocked ? "disabled" : ""}>${icon("plus")}</button>`;

  const placeholder = c.hasSelection ? "Describe a change, or pick an action" : "Ask or write anything…";

  root.innerHTML = `
    <div class="card${enterClass()}">
      ${header(selectionBadge(c))}
      <div class="body">
        <div class="command">
          <input id="cmd" class="input" dir="auto" data-autofocus placeholder="${placeholder}" autocomplete="off" spellcheck="false" ${blocked ? "disabled" : ""} />
          <div class="hints"><span class="kbd">↵</span><span class="kbd">esc</span></div>
        </div>
        ${c.hasSelection ? `<div class="preview" dir="auto">${esc(c.preview)}</div>` : ""}
        ${notice}
        ${
          c.hasSelection
            ? `<div class="chips">${actionChips}</div>
               <div class="section"><div class="caption">Translate to</div><div class="chips">${langChips}</div></div>`
            : ""
        }
      </div>
      <div class="foot">
        <span><span class="kbd">↑↓</span>move</span>
        <span><span class="kbd">↵</span>run</span>
        <span><span class="kbd">esc</span>close</span>
        <span class="spacer"></span>
        <span title="${esc(c.providerName)}">${esc(c.model)}</span>
      </div>
    </div>`;
  focusDefault();
}

function renderLangs(filter = "") {
  const c = ctx!;
  const q = filter.trim().toLowerCase();
  const list = LANGUAGES.filter((l) => !q || l.name.toLowerCase().includes(q) || l.code.toLowerCase() === q);
  root.innerHTML = `
    <div class="card${enterClass()}">
      ${header(selectionBadge(c))}
      <div class="body">
        <div class="command">
          <input id="lang-filter" class="input" data-autofocus placeholder="Translate to…" autocomplete="off" spellcheck="false" value="${esc(filter)}" />
          <div class="hints"><span class="kbd">esc</span></div>
        </div>
        <div class="lang-list">
          ${
            list.length
              ? list
                  .map(
                    (l) =>
                      `<button class="lang-item" data-lang="${esc(l.name)}"><span>${esc(l.name)}</span><span class="code">${esc(l.code)}</span></button>`,
                  )
                  .join("")
              : `<div class="muted" style="padding: 8px 10px">No match. Press Enter to translate to “${esc(filter)}”.</div>`
          }
        </div>
      </div>
    </div>`;
  const input = root.querySelector<HTMLInputElement>("#lang-filter")!;
  input.focus();
  input.setSelectionRange(input.value.length, input.value.length);
}

// ---------- generation ----------

async function run(action: string, instruction?: string, language?: string) {
  if (!ctx) return;
  const known = ctx.actions.find((a) => a.id === action);
  gen = {
    id: 0,
    label: action === "custom" ? "Custom" : "",
    kind: known?.kind ?? "transform",
    text: "",
    status: "streaming",
    error: "",
    tab: known?.diffFirst && ctx.hasSelection ? "changes" : "result",
    request: { action, instruction, language },
  };
  view = "result";
  renderResult();
  try {
    const started = await api.startGeneration(action, instruction, language);
    if (!gen) return;
    gen.id = started.id;
    gen.label = started.label;
    if (gen.kind !== started.kind) {
      gen.kind = started.kind;
      gen.tab = started.diffFirst && ctx.hasSelection ? "changes" : "result";
    }
    renderResult();
  } catch (e) {
    if (!gen) return;
    gen.status = "error";
    gen.error = errorMessage(e);
    renderResult();
  }
}

function onGen(ev: GenEvent) {
  if (!gen || view !== "result") return;
  if (gen.id !== 0 && gen.id !== ev.id) return;
  gen.id = ev.id;
  if (ev.kind === "delta") {
    gen.text += ev.text;
    const out = root.querySelector<HTMLElement>("#stream");
    if (out) {
      // The first words replace the loading skeleton.
      out.classList.remove("skeleton");
      out.textContent = gen.text;
      out.scrollTop = out.scrollHeight;
      syncGripSide();
    } else {
      renderResult();
    }
  } else if (ev.kind === "done") {
    gen.text = ev.text;
    gen.status = "done";
    renderResult();
  } else {
    gen.status = "error";
    gen.error = ev.text;
    renderResult();
  }
}

function primaryMode(): "replace" | "copy" {
  // Text you can't edit (a web page, a PDF) can't be replaced, so copy it.
  return gen?.kind === "answer" || !ctx?.canReplace || (ctx.hasSelection && !ctx.editable) ? "copy" : "replace";
}

function renderResult() {
  const c = ctx!;
  const g = gen!;
  // Same layout while writing and when done (tabs, output, info line, buttons),
  // so finishing only swaps the text and the popup never jumps.
  // A word-by-word comparison means nothing across languages, so not for Translate.
  const canDiff = c.hasSelection && g.kind === "transform" && g.request.action !== "translate";
  const status =
    g.status === "streaming"
      ? `<span class="status"><span class="spinner"></span>Writing…</span>`
      : g.status === "done"
        ? `<span class="status">${icon("check")}Ready</span>`
        : "";

  let body: string;
  if (g.status === "error") {
    body = `
      <div class="notice error">${icon("alert")}<span>${esc(g.error)}</span></div>
      <div class="actions-row">
        <button class="btn btn-sm btn-secondary" data-back>${icon("arrow-left")}Back</button>
        <span class="spacer"></span>
        <button class="btn btn-sm btn-outline" data-settings>Settings</button>
        <button class="btn btn-sm btn-primary" data-autofocus data-regen>${icon("refresh")}Try again</button>
      </div>`;
  } else {
    // dir="auto" lays out Arabic, Hebrew, Persian, Urdu… right-to-left.
    const textarea = `<textarea id="out" class="result" dir="auto" spellcheck="false">${esc(g.text)}</textarea>`;
    let inner: string;
    let metaText = "";
    if (g.status === "streaming") {
      inner = g.text
        ? `<div id="stream" class="result" dir="auto">${esc(g.text)}</div>`
        : // Nothing yet: a shimmering placeholder, laid out in the selection's direction.
          `<div id="stream" class="result skeleton" dir="${textDirection(c.text)}" aria-label="Loading">
             <span></span><span></span><span></span><span></span>
           </div>`;
      metaText = "Writing…";
    } else if (g.tab === "changes" && canDiff) {
      const d = renderDiff(c.text, g.text);
      inner = d ? `<div class="result" dir="auto">${d.html}</div>` : textarea;
      metaText = !d ? wordCount(g.text) : d.changes === 0 ? "No changes needed." : `${d.changes} ${d.changes === 1 ? "change" : "changes"}`;
    } else {
      inner = textarea;
      metaText = wordCount(g.text);
    }
    const meta = `<div class="result-meta">${esc(metaText)}</div>`;
    // Fixed height; drag the corner grip to make it bigger or smaller.
    const content = `
      <div class="result-wrap" id="result-wrap" style="height:${resultHeight}px">
        ${inner}
        <button class="grip" data-grip title="Drag to resize · double-click to reset" aria-label="Resize output">${icon("grip")}</button>
      </div>
      ${meta}`;

    const done = g.status === "done";
    const mode = primaryMode();
    const replaceLabel = c.hasSelection ? "Replace" : "Insert";
    const replaceBtn = c.canReplace
      ? `<button class="btn btn-sm ${mode === "replace" ? "btn-primary" : "btn-outline"}" data-apply="replace" ${done ? "" : "disabled"} ${mode === "replace" ? "data-autofocus" : ""}>
           ${icon("corner-down-left")}${replaceLabel}${mode === "replace" ? `<span class="primary-hint">Ctrl↵</span>` : ""}</button>`
      : "";
    const copyBtn = `<button class="btn btn-sm ${mode === "copy" ? "btn-primary" : "btn-outline"}" data-apply="copy" ${done ? "" : "disabled"} ${mode === "copy" ? "data-autofocus" : ""}>
           ${icon("copy")}Copy${mode === "copy" ? `<span class="primary-hint">Ctrl↵</span>` : ""}</button>`;

    body = `
      ${g.note ? `<div class="notice">${icon("info")}<span>${esc(g.note)}</span></div>` : ""}
      ${
        canDiff
          ? `<div class="tabs">
               <button class="tab ${g.tab === "result" ? "active" : ""}" data-tab="result" ${done ? "" : "disabled"}>Result</button>
               <button class="tab ${g.tab === "changes" ? "active" : ""}" data-tab="changes" ${done ? "" : "disabled"}>Changes</button>
             </div>`
          : ""
      }
      ${content}
      <div class="actions-row">
        <button class="btn btn-sm btn-secondary" data-back title="Back to actions">${icon("arrow-left")}Back</button>
        <span class="spacer"></span>
        <button class="icon-btn" data-regen title="Regenerate (Ctrl+R)" ${g.status === "streaming" ? "disabled" : ""}>${icon("refresh")}</button>
        ${mode === "copy" ? replaceBtn + copyBtn : copyBtn + replaceBtn}
      </div>`;
  }

  root.innerHTML = `
    <div class="card${enterClass()}">
      ${header(`<span class="badge badge-solid">${esc(g.label || "…")}</span>${status}`)}
      <div class="body">${body}</div>
    </div>`;

  const out = root.querySelector<HTMLTextAreaElement>("#out");
  out?.addEventListener("input", () => {
    g.text = out.value;
    syncGripSide();
  });
  syncGripSide();
  if (g.status !== "streaming") focusDefault();
}

/** "rtl" when the first letter of the text is from a right-to-left script. */
function textDirection(text: string): "rtl" | "ltr" {
  const first = text.match(/[A-Za-zÀ-ɏͰ-ϿЀ-ӿ֐-ࣿיִ-﷿ﹰ-﻿]/);
  return first && /[֐-ࣿיִ-﷿ﹰ-﻿]/.test(first[0]) ? "rtl" : "ltr";
}

function wordCount(text: string): string {
  const n = text.trim() ? text.trim().split(/\s+/).length : 0;
  return `${n} ${n === 1 ? "word" : "words"}`;
}

// ---------- resizable output ----------

const DEFAULT_RESULT_HEIGHT = 140;
const MIN_RESULT_HEIGHT = 80;
const MAX_RESULT_HEIGHT = 640;

/** The size you drag to lasts for this popup only; a new one starts fresh. */
let resultHeight = DEFAULT_RESULT_HEIGHT;

function setResultHeight(h: number) {
  resultHeight = Math.round(Math.min(MAX_RESULT_HEIGHT, Math.max(MIN_RESULT_HEIGHT, h)));
  const wrap = root.querySelector<HTMLElement>("#result-wrap");
  if (wrap) wrap.style.height = `${resultHeight}px`;
}

/**
 * Right-to-left text puts the scrollbar on the left, so the grip moves with
 * it and follows the text as it changes.
 */
function syncGripSide() {
  const wrap = root.querySelector<HTMLElement>("#result-wrap");
  const out = wrap?.querySelector<HTMLElement>(".result");
  if (wrap && out) wrap.classList.toggle("rtl", out.matches(":dir(rtl)"));
}

root.addEventListener("pointerdown", (e) => {
  const grip = (e.target as HTMLElement).closest<HTMLElement>("[data-grip]");
  if (!grip || e.button !== 0) return;
  e.preventDefault();
  const startY = e.screenY;
  const startH = resultHeight;
  grip.setPointerCapture(e.pointerId);
  // Grow the window once, up front, to the largest size the drag can reach,
  // so dragging only changes the page and never waits on the window.
  gripDragging = true;
  const size = measure();
  // Until the window reports how far it can grow, only allow shrinking.
  let limit = startH;
  void api.popupBeginResize(size.w, size.h + (MAX_RESULT_HEIGHT - resultHeight)).then((granted) => {
    if (Number.isFinite(granted)) limit = startH + Math.max(0, Math.floor(granted - size.h));
  });
  let frame = 0;
  let wanted = startH;
  const move = (ev: PointerEvent) => {
    wanted = Math.min(limit, startH + (ev.screenY - startY));
    if (!frame) {
      frame = requestAnimationFrame(() => {
        frame = 0;
        setResultHeight(wanted);
      });
    }
  };
  const up = () => {
    grip.removeEventListener("pointermove", move);
    grip.removeEventListener("pointerup", up);
    grip.removeEventListener("pointercancel", up);
    cancelAnimationFrame(frame);
    setResultHeight(wanted);
    // Shrink the window back around the card.
    gripDragging = false;
    const final = measure();
    lastSize = final;
    void api.popupResize(final.w, final.h);
  };
  grip.addEventListener("pointermove", move);
  grip.addEventListener("pointerup", up);
  grip.addEventListener("pointercancel", up);
});

root.addEventListener("dblclick", (e) => {
  if ((e.target as HTMLElement).closest("[data-grip]")) setResultHeight(DEFAULT_RESULT_HEIGHT);
});

async function apply(mode: "replace" | "copy") {
  if (!gen || gen.status !== "done") return;
  const text = gen.text.trim();
  if (!text) return;
  try {
    await api.applyResult(text, mode);
  } catch (e) {
    gen.status = "error";
    gen.error = errorMessage(e);
    renderResult();
  }
}

function close() {
  gen = null;
  void api.popupHide();
}

// ---------- toast ----------

let lastToastState: ToastPayload["state"] | null = null;

function showToast(t: ToastPayload) {
  // A finished toast updates the "working" one in place instead of popping in again.
  const enter = t.state === "working" || lastToastState !== "working" || view !== "toast" ? " enter" : "";
  lastToastState = t.state;
  view = "toast";
  const inner =
    t.state === "working"
      ? `<div class="toast${enter}" data-toast-cancel title="Click to cancel"><span class="spinner"></span><span>${esc(t.text)}</span><span class="muted toast-hint">Click to cancel</span></div>`
      : t.state === "done"
        ? `<div class="toast${enter}"><span class="check">${icon("check")}</span><span>${esc(t.text)}</span></div>`
        : `<div class="toast error${enter}" data-toast-settings title="Open Settings">${icon("alert")}<span class="msg">${esc(t.text)}</span></div>`;
  root.innerHTML = inner;
  // The app hides finished toasts itself after a short delay.
  void present(t.id, false, "toast");
}

// ---------- input handling ----------

root.addEventListener("click", (e) => {
  const el = (e.target as HTMLElement).closest<HTMLElement>("button, [data-toast-settings], [data-toast-cancel]");
  if (!el) return;
  if (el.hasAttribute("data-toast-cancel")) return void api.cancelDirect();
  if (el.hasAttribute("data-close")) return close();
  if (el.hasAttribute("data-open-settings")) return void api.showSettings();
  if (el.hasAttribute("data-settings") || el.hasAttribute("data-toast-settings")) {
    return void api.showSettings(ctx?.problem || el.hasAttribute("data-toast-settings") ? "provider" : undefined);
  }
  if (el.dataset.action) return void run(el.dataset.action);
  if (el.dataset.lang) return void run("translate", undefined, el.dataset.lang);
  if (el.hasAttribute("data-more")) {
    view = "langs";
    return renderLangs();
  }
  if (el.hasAttribute("data-back")) {
    void api.cancelGeneration();
    gen = null;
    view = "menu";
    return renderMenu();
  }
  if (el.hasAttribute("data-regen") && gen) {
    const r = gen.request;
    return void run(r.action, r.instruction, r.language);
  }
  if (el.dataset.tab && gen) {
    gen.tab = el.dataset.tab as Generation["tab"];
    return renderResult();
  }
  if (el.dataset.apply) return void apply(el.dataset.apply as "replace" | "copy");
});

// Drag the popup by its header (anywhere except its buttons).
root.addEventListener("mousedown", (e) => {
  const t = e.target as HTMLElement;
  if (e.button !== 0 || !t.closest(".head") || t.closest("button")) return;
  e.preventDefault();
  void api.popupDrag();
});

root.addEventListener("input", (e) => {
  const t = e.target as HTMLInputElement;
  if (t.id === "lang-filter") renderLangs(t.value);
});

function moveFocus(delta: number) {
  const items = [...root.querySelectorAll<HTMLElement>("#cmd, #lang-filter, .chip:not(:disabled), .lang-item")];
  const i = items.indexOf(document.activeElement as HTMLElement);
  const next = items[Math.max(0, Math.min(items.length - 1, i + delta))];
  next?.focus();
}

window.addEventListener("keydown", (e) => {
  if (view === "toast") return;
  const active = document.activeElement as HTMLElement | null;

  if (e.key === "Escape") {
    e.preventDefault();
    if (view === "langs") {
      view = "menu";
      return renderMenu();
    }
    return close();
  }

  if (view === "result") {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      return void apply(primaryMode());
    }
    if (e.key.toLowerCase() === "r" && e.ctrlKey && gen && gen.status !== "streaming") {
      e.preventDefault();
      const r = gen.request;
      return void run(r.action, r.instruction, r.language);
    }
    return;
  }

  if (e.key === "Enter" && active?.id === "cmd") {
    e.preventDefault();
    const value = (active as HTMLInputElement).value.trim();
    if (value) void run("custom", value);
    return;
  }
  if (e.key === "Enter" && active?.id === "lang-filter") {
    e.preventDefault();
    const first = root.querySelector<HTMLElement>(".lang-item");
    const value = (active as HTMLInputElement).value.trim();
    const lang = first?.dataset.lang ?? value;
    if (lang) void run("translate", undefined, lang);
    return;
  }

  const inText = active?.tagName === "INPUT";
  if (e.key === "ArrowDown" || (e.key === "ArrowRight" && !inText)) {
    e.preventDefault();
    moveFocus(1);
  } else if (e.key === "ArrowUp" || (e.key === "ArrowLeft" && !inText)) {
    e.preventDefault();
    moveFocus(-1);
  } else if (!inText && e.key.length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey) {
    // Start typing anywhere to write an instruction.
    root.querySelector<HTMLInputElement>("#cmd, #lang-filter")?.focus();
  }
});

// ---------- wiring ----------

void listen<OpenPayload>("popup-open", (e) => openMenu(e.payload));
void listen<ToastPayload>("toast", (e) => showToast(e.payload));
void listen<GenEvent>("gen", (e) => onGen(e.payload));
void listen("popup-clear", () => {
  gen = null;
  lastToastState = null;
  lastSize = { w: 0, h: 0 };
  root.innerHTML = "";
});

// If a request arrived before this page finished loading, pick it up now.
void api.popupSync().then((p) => {
  if (p) openMenu(p);
});
