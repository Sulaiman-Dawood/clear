// A dropdown styled like the rest of the app, with a search box.
//
// It enhances existing <select class="select"> elements: the native select
// stays in the page (hidden) and keeps the value, and choosing an option
// sets it and fires a normal "change" event, so page code works unchanged.
import { esc } from "./dom";
import { icon } from "./icons";

interface Option {
  value: string;
  label: string;
}

interface Open {
  select: HTMLSelectElement;
  trigger: HTMLButtonElement;
  options: Option[];
  shown: Option[];
  active: number;
}

/** Lists longer than this get a search box (every list, for now). */
const SEARCH_FROM = 0;

let panel: HTMLElement | null = null;
let state: Open | null = null;

export function enhanceSelects(root: ParentNode): void {
  root.querySelectorAll<HTMLSelectElement>("select.select:not([data-enhanced])").forEach((select) => {
    select.dataset.enhanced = "1";
    select.hidden = true;

    const trigger = document.createElement("button");
    trigger.type = "button";
    trigger.className = `${select.className} dd-trigger`;
    trigger.setAttribute("aria-haspopup", "listbox");
    trigger.setAttribute("aria-expanded", "false");
    if (select.id) {
      trigger.id = `${select.id}-dropdown`;
      // Clicking the field's label opens the dropdown.
      document.querySelectorAll<HTMLLabelElement>(`label[for="${CSS.escape(select.id)}"]`).forEach((l) => (l.htmlFor = trigger.id));
    }
    setTriggerText(trigger, select);
    trigger.addEventListener("click", () => (state?.select === select ? close() : open(select, trigger)));
    trigger.addEventListener("keydown", (e) => {
      if (["ArrowDown", "ArrowUp", "Enter", " "].includes(e.key)) {
        e.preventDefault();
        open(select, trigger);
      }
    });
    select.after(trigger);
  });
}

function setTriggerText(trigger: HTMLButtonElement, select: HTMLSelectElement) {
  const chosen = select.selectedOptions[0];
  const placeholder = !chosen || chosen.disabled;
  trigger.innerHTML = `<span class="dd-value ${placeholder ? "dd-placeholder" : ""}">${esc(chosen?.textContent ?? "")}</span>`;
}

function open(select: HTMLSelectElement, trigger: HTMLButtonElement) {
  close();
  const options = [...select.options].filter((o) => !o.disabled).map((o) => ({ value: o.value, label: o.textContent ?? o.value }));
  const searchable = options.length > SEARCH_FROM;

  panel = document.createElement("div");
  panel.className = "dd-panel";
  panel.setAttribute("role", "listbox");
  panel.tabIndex = -1;
  panel.innerHTML = `
    ${
      searchable
        ? `<div class="dd-search">${icon("search")}<input class="dd-input" placeholder="Search…" autocomplete="off" spellcheck="false" aria-label="Search options" /></div>`
        : ""
    }
    <div class="dd-list"></div>`;
  document.body.appendChild(panel);

  state = { select, trigger, options, shown: options, active: Math.max(0, options.findIndex((o) => o.value === select.value)) };
  trigger.setAttribute("aria-expanded", "true");
  trigger.classList.add("dd-open");
  renderList();
  position();

  const input = panel.querySelector<HTMLInputElement>(".dd-input");
  input?.addEventListener("input", () => filter(input.value));
  panel.addEventListener("keydown", onKey);
  panel.addEventListener("mousemove", (e) => {
    const i = Number((e.target as HTMLElement).closest<HTMLElement>("[data-i]")?.dataset.i);
    if (Number.isInteger(i) && state && i !== state.active) {
      state.active = i;
      markActive(false);
    }
  });
  panel.addEventListener("click", (e) => {
    const i = Number((e.target as HTMLElement).closest<HTMLElement>("[data-i]")?.dataset.i);
    if (Number.isInteger(i)) choose(i);
  });
  (input ?? panel).focus();
  markActive(true);
}

function filter(query: string) {
  if (!state) return;
  const q = query.trim().toLowerCase();
  state.shown = q
    ? state.options.filter((o) => o.label.toLowerCase().includes(q) || o.value.toLowerCase().includes(q))
    : state.options;
  state.active = 0;
  renderList();
  markActive(true);
}

function renderList() {
  if (!panel || !state) return;
  const list = panel.querySelector<HTMLElement>(".dd-list")!;
  const { shown, select } = state;
  list.innerHTML = shown.length
    ? shown
        .map(
          (o, i) => `<div class="dd-option" role="option" data-i="${i}" aria-selected="${o.value === select.value}" dir="auto">
            <span class="dd-label">${esc(o.label)}</span>${o.value === select.value ? icon("check", "icon dd-check") : ""}
          </div>`,
        )
        .join("")
    : `<div class="dd-empty">No matches</div>`;
}

function markActive(scroll: boolean) {
  if (!panel || !state) return;
  panel.querySelectorAll(".dd-option.active").forEach((el) => el.classList.remove("active"));
  const el = panel.querySelector<HTMLElement>(`[data-i="${state.active}"]`);
  el?.classList.add("active");
  if (scroll) el?.scrollIntoView({ block: "nearest" });
}

function onKey(e: KeyboardEvent) {
  if (!state) return;
  const last = state.shown.length - 1;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    if (last < 0) return;
    const step = e.key === "ArrowDown" ? 1 : -1;
    state.active = (state.active + step + last + 1) % (last + 1);
    markActive(true);
  } else if (e.key === "Home" || e.key === "End") {
    if ((e.target as HTMLElement).classList.contains("dd-input")) return;
    e.preventDefault();
    state.active = e.key === "Home" ? 0 : Math.max(0, last);
    markActive(true);
  } else if (e.key === "Enter") {
    e.preventDefault();
    if (last >= 0) choose(state.active);
  } else if (e.key === "Escape") {
    e.preventDefault();
    e.stopPropagation();
    const trigger = state.trigger;
    close();
    trigger.focus();
  } else if (e.key === "Tab") {
    close();
  }
}

function choose(i: number) {
  if (!state) return;
  const option = state.shown[i];
  if (!option) return;
  const { select, trigger } = state;
  close();
  if (select.value !== option.value) {
    select.value = option.value;
    setTriggerText(trigger, select);
    select.dispatchEvent(new Event("change", { bubbles: true }));
  }
  if (trigger.isConnected) trigger.focus();
}

function position() {
  if (!panel || !state) return;
  const r = state.trigger.getBoundingClientRect();
  const width = Math.max(r.width, 220);
  const left = Math.min(r.left, window.innerWidth - width - 8);
  panel.style.width = `${width}px`;
  panel.style.left = `${Math.max(8, left)}px`;
  const height = panel.offsetHeight;
  const below = window.innerHeight - r.bottom - 8;
  const above = r.top - 8;
  const top = height + 6 <= below || below >= above ? r.bottom + 6 : r.top - 6 - Math.min(height, above);
  panel.style.top = `${top}px`;
  panel.style.maxHeight = `${Math.max(160, height + 6 <= below || below >= above ? below - 6 : above - 6)}px`;
}

export function close(): void {
  if (state) {
    state.trigger.setAttribute("aria-expanded", "false");
    state.trigger.classList.remove("dd-open");
  }
  panel?.remove();
  panel = null;
  state = null;
}

// Close when clicking elsewhere, scrolling the page, or resizing the window.
document.addEventListener(
  "mousedown",
  (e) => {
    if (!state) return;
    const t = e.target as Node;
    if (panel?.contains(t) || state.trigger.contains(t)) return;
    close();
  },
  true,
);
document.addEventListener(
  "scroll",
  (e) => {
    if (state && !panel?.contains(e.target as Node)) close();
  },
  true,
);
window.addEventListener("resize", () => close());
window.addEventListener("blur", () => close());
