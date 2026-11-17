// The Settings window.
import "./lib/boot";
import "@fontsource-variable/geist";
import "./styles/settings.css";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, errorMessage, type AppInfo, type HistoryEntry, type ProviderPreset, type Settings } from "./lib/ipc";
import { icon, logo } from "./lib/icons";
import { $, esc, formatAccelerator, lockDownWebview } from "./lib/dom";
import { LANGUAGES } from "./lib/languages";
import { close as closeDropdown, enhanceSelects } from "./lib/dropdown";
import { applyTheme, initTheme, type Theme } from "./lib/theme";

lockDownWebview();
initTheme();

type Section = "general" | "actions" | "shortcuts" | "provider" | "history" | "about";

const SECTIONS: { id: Section; label: string; icon: string }[] = [
  { id: "general", label: "General", icon: "sliders" },
  { id: "actions", label: "Actions", icon: "grid" },
  { id: "shortcuts", label: "Shortcuts", icon: "keyboard" },
  { id: "provider", label: "AI provider", icon: "cpu" },
  { id: "history", label: "History", icon: "history" },
  { id: "about", label: "About", icon: "info" },
];

const GROUP_LABELS: Record<string, string> = {
  tone: "Tone",
  structure: "Structure",
  creative: "Creative",
};

let info: AppInfo;
let section: Section = "general";
let history: HistoryEntry[] = [];
let recording: string | null = null;
let recordError = "";
let testState: { status: "idle" | "running" | "ok" | "error"; text: string } = { status: "idle", text: "" };
const loadedModels: Record<string, string[]> = {};
let modelsLoading = false;
let modelsError = "";
let resetArmed = false;

const settings = (): Settings => info.settings;
const activePreset = (): ProviderPreset =>
  info.providers.find((p) => p.id === settings().activeProvider) ?? info.providers[0];
const profile = () => settings().providers[settings().activeProvider] ?? { baseUrl: "", model: "" };

function needsSetup(): boolean {
  const p = activePreset();
  return (p.needsKey && !info.keyStatus[p.id]) || !profile().model.trim() || !profile().baseUrl.trim();
}

// ---------- persistence ----------

let saveTimer: number | undefined;
function save(delay = 250) {
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(async () => {
    try {
      info.settings = await api.saveSettings(settings());
      flash("Saved");
      renderSidebar();
    } catch (e) {
      flash(errorMessage(e));
    }
  }, delay);
}

let flashTimer: number | undefined;
function flash(text: string) {
  const el = $("#flash");
  el.textContent = text;
  el.classList.add("show");
  window.clearTimeout(flashTimer);
  flashTimer = window.setTimeout(() => el.classList.remove("show"), 1600);
}

// ---------- rendering ----------

function renderSidebar() {
  $("#sidebar").innerHTML = `
    <div class="brand">${logo(28)}<span>Clear</span></div>
    ${SECTIONS.map(
      (s) => `<button class="nav-item ${s.id === section ? "active" : ""}" data-nav="${s.id}">
        ${icon(s.icon)}<span>${s.label}</span>${s.id === "provider" && needsSetup() ? '<span class="warn" title="Setup needed"></span>' : ""}
      </button>`,
    ).join("")}
    <div class="foot">
      <button class="btn btn-sm btn-outline" data-open-menu>${icon("sparkles")}Open Clear</button>
      <span>v${esc(info.version)}</span>
    </div>`;
}

function render() {
  renderSidebar();
  const content = $("#content");
  const renderers: Record<Section, () => string> = {
    general: renderGeneral,
    actions: renderActions,
    shortcuts: renderShortcuts,
    provider: renderProvider,
    history: renderHistory,
    about: renderAbout,
  };
  closeDropdown();
  content.innerHTML = renderers[section]();
  enhanceSelects(content);
}

function go(next: Section) {
  if (recording) void stopRecording();
  section = next;
  resetArmed = false;
  if (next === "history") void refreshHistory();
  render();
  document.querySelector(".main")?.scrollTo({ top: 0 });
}

function languageOptions(selected: string) {
  const names = LANGUAGES.map((l) => l.name);
  if (!names.includes(selected)) names.unshift(selected);
  return names.map((n) => `<option ${n === selected ? "selected" : ""}>${esc(n)}</option>`).join("");
}

function accel(action: string) {
  return settings().hotkeys[action] || "";
}

function renderGeneral(): string {
  const p = activePreset();
  const setup = needsSetup();
  const menuKey = accel("menu");
  const fixKey = accel("fix");
  return `
    <h1>General</h1>
    <p class="lead">Select text in any app, press a shortcut, and get better text back in place.</p>

    ${
      setup
        ? `<div class="banner">
             ${icon("key")}
             <div class="text"><div>Finish setting up your AI provider</div>
               <div class="desc">Clear uses your own API key or a local model. Nothing works until one is set.</div></div>
             <button class="btn btn-sm" data-nav="provider">Set up</button>
           </div>`
        : ""
    }

    <div class="card stack">
      <div class="inline">
        <span class="badge ${setup ? "badge-outline" : "badge-solid"}"><span class="dot"></span>${setup ? "Setup needed" : "Ready"}</span>
        <span class="muted">${esc(p.name)} · <span class="mono">${esc(profile().model || "no model")}</span></span>
        <span class="spacer" style="flex:1"></span>
        <button class="btn btn-sm btn-outline" data-test ${setup ? "disabled" : ""}>Test connection</button>
      </div>
      ${testLine()}
    </div>

    <div class="caption section-title">How to use it</div>
    <div class="card">
      <div class="steps">
        <div class="step"><span class="n">01</span><strong>Select text</strong><span class="muted">In any app: email, chat, docs, browser.</span></div>
        <div class="step"><span class="n">02</span><strong>Open the menu</strong>
          <span class="keys">${menuKey ? formatAccelerator(menuKey) : '<span class="muted">No shortcut set</span>'}</span>
          <span class="muted">Pick an action, or type your own instruction.</span></div>
        <div class="step"><span class="n">03</span><strong>Or fix instantly</strong>
          <span class="keys">${fixKey ? formatAccelerator(fixKey) : '<span class="muted">No shortcut set</span>'}</span>
          <span class="muted">Fixes the selection in place, no menu.</span></div>
      </div>
    </div>

    <div class="caption section-title">Defaults</div>
    <div class="card flush">
      <div class="row">
        <div class="text"><div class="title">Quick languages</div>
          <div class="desc">One-click translate buttons in the menu. The Translate shortcut uses the first, or the second when the text is already in the first.</div></div>
      </div>
      <div class="grid-3" style="padding: 0 0 16px">
        ${settings()
          .quickLanguages.map(
            (l, i) => `<select class="select" data-quick="${i}">${languageOptions(l)}</select>`,
          )
          .join("")}
      </div>
    </div>

    <div class="caption section-title">Behavior</div>
    <div class="card flush">
      <div class="row">
        <div class="text"><div class="title">Appearance</div>
          <div class="desc">System follows your Windows light or dark mode.</div></div>
        <div class="control">
          <div class="segmented">
            ${(["system", "light", "dark"] as const)
              .map(
                (t) =>
                  `<button data-theme-choice="${t}" class="${settings().theme === t ? "active" : ""}">${t[0].toUpperCase() + t.slice(1)}</button>`,
              )
              .join("")}
          </div>
        </div>
      </div>
      <div class="row">
        <div class="text"><div class="title">Launch at login</div>
          <div class="desc">Start Clear quietly in the tray when you sign in to Windows.</div></div>
        <div class="control">${toggle("launchAtLogin", settings().launchAtLogin)}</div>
      </div>
    </div>`;
}

function toggle(field: string, on: boolean) {
  return `<label class="switch"><input type="checkbox" data-toggle="${field}" ${on ? "checked" : ""} /><span></span></label>`;
}

function testLine(): string {
  if (testState.status === "idle") return "";
  if (testState.status === "running")
    return `<div class="status-line"><span class="spinner"></span>Testing…</div>`;
  if (testState.status === "ok") return `<div class="status-line ok">${icon("check")}${esc(testState.text)}</div>`;
  return `<div class="status-line error">${icon("alert")}${esc(testState.text)}</div>`;
}

function renderActions(): string {
  const core = info.catalog.filter((a) => a.group === "core");
  const extras = info.catalog.filter((a) => a.group !== "core");
  const groups = [...new Set(extras.map((a) => a.group))];
  const enabled = new Set(settings().enabledExtras);
  const disabled = new Set(settings().disabledActions);
  const presets = settings().customPresets;
  return `
    <h1>Actions</h1>
    <p class="lead">Choose what shows up in the menu. To run any action straight from a key combination, give it a shortcut in Shortcuts.</p>

    <div class="caption section-title">Default actions</div>
    <div class="card flush">
      ${core
        .map(
          (a) => `<div class="row">
            <span class="row-icon">${icon(a.icon)}</span>
            <div class="text"><div class="title">${esc(a.label)}</div><div class="desc">${esc(a.description)}</div></div>
            <div class="control"><label class="switch"><input type="checkbox" data-core="${esc(a.id)}" ${disabled.has(a.id) ? "" : "checked"} /><span></span></label></div>
          </div>`,
        )
        .join("")}
    </div>

    ${groups
      .map(
        (g) => `
      <div class="caption section-title">${esc(GROUP_LABELS[g] ?? g)}</div>
      <div class="card flush">
        ${extras
          .filter((a) => a.group === g)
          .map(
            (a) => `<div class="row">
              <span class="row-icon">${icon(a.icon)}</span>
              <div class="text"><div class="title">${esc(a.label)}</div><div class="desc">${esc(a.description)}</div></div>
              <div class="control"><label class="switch"><input type="checkbox" data-extra="${esc(a.id)}" ${enabled.has(a.id) ? "checked" : ""} /><span></span></label></div>
            </div>`,
          )
          .join("")}
      </div>`,
      )
      .join("")}

    <div class="caption section-title">Your presets</div>
    ${
      presets.length
        ? `<div class="card flush">${presets
            .map(
              (p) => `<div class="row">
                <span class="row-icon">${icon("wand")}</span>
                <div class="text"><div class="title">${esc(p.label)}</div><div class="desc">${esc(p.instruction)}</div></div>
                <div class="control"><button class="icon-btn" data-delete-preset="${esc(p.id)}" title="Delete preset">${icon("trash")}</button></div>
              </div>`,
            )
            .join("")}</div>`
        : `<div class="empty">No presets yet. Add one below and it appears as a button in the menu.</div>`
    }
    <div class="card stack">
      <h2>Add a preset</h2>
      <div class="field">
        <label for="preset-label">Button label</label>
        <input id="preset-label" class="input" maxlength="30" placeholder="e.g. Release notes" />
      </div>
      <div class="field">
        <label for="preset-instruction">Instruction</label>
        <textarea id="preset-instruction" class="textarea" rows="3" placeholder="e.g. Rewrite as short, upbeat release notes with one bullet per change."></textarea>
      </div>
      <div class="inline"><button class="btn btn-primary" data-add-preset>${icon("plus")}Add preset</button></div>
    </div>`;
}

interface ShortcutTarget {
  id: string;
  label: string;
  description: string;
  icon: string;
  /** Edits can replace in place or open the popup; answers always open the popup. */
  canReplace: boolean;
}

/** Same list and default behavior as `shortcut_targets` / `shortcut_mode` in actions.rs. */
function shortcutTargets(): ShortcutTarget[] {
  return [
    { id: "menu", label: "Action menu", description: "Opens the popup with every action. Works with or without a selection.", icon: "grid", canReplace: false },
    { id: "translate", label: "Translate", description: "Your first quick language, or the second if the text is already in the first.", icon: "globe", canReplace: true },
    ...info.catalog.map((a) => ({ id: a.id, label: a.label, description: a.description, icon: a.icon, canReplace: a.kind === "transform" })),
    ...settings().customPresets.map((p) => ({ id: `preset:${p.id}`, label: p.label, description: p.instruction, icon: "wand", canReplace: true })),
  ];
}

function shortcutMode(t: ShortcutTarget): "replace" | "popup" {
  if (!t.canReplace) return "popup";
  return settings().hotkeyModes[t.id] ?? (t.id === "translate" ? "popup" : "replace");
}


function renderShortcuts(): string {
  const targets = shortcutTargets();
  // Only actions with a shortcut get a row (plus the one being added right now).
  const shown = targets.filter((t) => accel(t.id) || recording === t.id);
  const available = targets.filter((t) => !shown.includes(t));

  const row = (t: ShortcutTarget) => {
    const value = accel(t.id);
    const isRec = recording === t.id;
    const error = isRec ? recordError : info.hotkeyErrors[t.id];
    const mode = shortcutMode(t);
    const modeControl =
      t.id === "menu"
        ? ""
        : t.canReplace
          ? `<div class="segmented sm" title="What the shortcut does with the result">
               <button class="${mode === "replace" ? "active" : ""}" data-mode="${esc(t.id)}" data-value="replace">In place</button>
               <button class="${mode === "popup" ? "active" : ""}" data-mode="${esc(t.id)}" data-value="popup">Popup</button>
             </div>`
          : `<span class="sc-fixed" title="Answers always open in the popup">Popup</span>`;
    const note = error ? `<div class="row-error">${esc(error)}</div>` : "";
    return `<div class="sc-row">
      <div class="sc-name" title="${esc(t.description)}">
        <span class="sc-icon">${icon(t.icon)}</span><span>${esc(t.label)}</span>
      </div>
      <div class="sc-controls">
        ${modeControl}
        <button class="hotkey-btn ${isRec ? "recording" : ""}" data-record="${esc(t.id)}" title="Click, then press new keys">
          ${isRec ? "Press keys…" : formatAccelerator(value)}
        </button>
        <button class="icon-btn" data-clear-hotkey="${esc(t.id)}" title="Remove this shortcut" ${value ? "" : "disabled"}>${icon("x")}</button>
      </div>
      ${note ? `<div class="sc-note">${note}</div>` : ""}
    </div>`;
  };

  return `
    <h1>Shortcuts</h1>
    <p class="lead">Click a shortcut to change it, or add one for any action. <strong>In place</strong> replaces
      your selection; <strong>Popup</strong> shows the result first.</p>
    <div class="card sc-card">
      ${shown.length ? shown.map(row).join("") : `<div class="sc-empty">No shortcuts yet.</div>`}
      ${
        available.length
          ? `<div class="sc-add">
               <select class="select" data-add-shortcut aria-label="Add a shortcut">
                 <option value="" selected disabled>Add a shortcut…</option>
                 ${available.map((t) => `<option value="${esc(t.id)}">${esc(t.label)}</option>`).join("")}
               </select>
             </div>`
          : ""
      }
    </div>
    <div class="inline"><button class="link-btn" data-reset-hotkeys>${icon("refresh")}Reset to default shortcuts</button></div>`;
}

const OTHER_MODEL = "__other__";
let editingCustomModel = false;
const autoLoaded = new Set<string>();

/** The current model always appears in the list, even if the provider didn't return it. */
function modelOptions(list: string[], current: string): string[] {
  return current && !list.includes(current) ? [current, ...list] : list;
}

/** Fetch the model list once per provider when it can work (key saved, or local). */
function maybeAutoLoadModels() {
  const p = activePreset();
  const ready = p.local || !p.needsKey ? !!profile().baseUrl : !!info.keyStatus[p.id];
  if (ready && !loadedModels[p.id] && !autoLoaded.has(p.id) && !modelsLoading) {
    autoLoaded.add(p.id);
    void loadModels();
  }
}

function renderProvider(): string {
  const p = activePreset();
  const prof = profile();
  const hasKey = !!info.keyStatus[p.id];
  const suggestions = [...new Set([...(loadedModels[p.id] ?? []), ...p.suggestedModels])];
  queueMicrotask(maybeAutoLoadModels);
  const keyBlock = p.local
    ? `<div class="status-line ok">${icon("check")}No key needed. Runs on this computer.</div>`
    : `
      <div class="field">
        <label for="api-key">API key${p.needsKey ? "" : " (optional)"}</label>
        ${
          hasKey
            ? `<div class="inline">
                 <span class="badge badge-soft">${icon("key", "icon")}Saved in Windows Credential Manager</span>
                 <span style="flex:1"></span>
                 <button class="btn btn-sm btn-danger" data-clear-key>Remove key</button>
               </div>`
            : ""
        }
        <div class="inline">
          <input id="api-key" class="input mono" type="password" autocomplete="off" spellcheck="false" placeholder="${hasKey ? "Paste a new key to replace it" : "Paste your API key"}" />
          <button class="btn btn-primary" data-save-key>Save key</button>
        </div>
        <span class="hint">${esc(p.keyHint)}. The key is stored by Windows, never in a file, and is only sent to ${esc(p.name)}.</span>
      </div>`;

  return `
    <h1>AI provider</h1>
    <p class="lead">Clear has no server. Your text goes straight from this computer to the provider you choose.</p>

    <div class="card stack">
      <div class="field">
        <label for="provider">Provider</label>
        <select id="provider" class="select" data-provider>
          ${info.providers.map((x) => `<option value="${x.id}" ${x.id === p.id ? "selected" : ""}>${esc(x.name)}</option>`).join("")}
        </select>
      </div>

      ${keyBlock}

      <div class="field">
        <div class="label-row">
          <label for="model">Model</label>
          <button class="link-btn" data-load-models ${modelsLoading ? "disabled" : ""}>
            ${modelsLoading ? '<span class="spinner"></span>Loading…' : `${icon("refresh")}Refresh list`}
          </button>
        </div>
        <select id="model" class="select" data-model>
          ${prof.model ? "" : '<option value="" selected disabled>Choose a model</option>'}
          ${modelOptions(suggestions, prof.model)
            .map((m) => `<option value="${esc(m)}" ${m === prof.model && !editingCustomModel ? "selected" : ""}>${esc(m)}</option>`)
            .join("")}
          <option value="${OTHER_MODEL}" ${editingCustomModel ? "selected" : ""}>Other model id…</option>
        </select>
        ${
          editingCustomModel
            ? `<input id="model-custom" class="input mono" data-profile="model" value="${esc(prof.model)}" placeholder="Type the exact model id, then press Enter" spellcheck="false" />`
            : ""
        }
        ${
          modelsError
            ? `<span class="hint error-text">${esc(modelsError)}</span>`
            : !suggestions.length
              ? `<span class="hint">${hasKey || p.local || !p.needsKey ? "Click Refresh list to load the models this provider offers." : "Save your API key to load the models this provider offers."}</span>`
              : p.wire === "anthropic"
                ? `<span class="hint">Opus is the most capable. Haiku is the fastest and cheapest, a good fit for quick fixes.</span>`
                : ""
        }
      </div>

      ${
        p.wire === "anthropic"
          ? `<div class="field">
               <label>Effort</label>
               <div class="segmented">
                 ${(["low", "medium", "high"] as const)
                   .map(
                     (e) =>
                       `<button data-effort="${e}" class="${settings().anthropicEffort === e ? "active" : ""}">${e[0].toUpperCase() + e.slice(1)}</button>`,
                   )
                   .join("")}
               </div>
               <span class="hint">How long the model thinks before answering. Low is fastest and fine for most edits.</span>
             </div>`
          : ""
      }

      <div class="field">
        <label for="base-url">Server URL</label>
        <input id="base-url" class="input mono" data-profile="baseUrl" value="${esc(prof.baseUrl)}" placeholder="https://…" spellcheck="false" />
        <span class="hint">${p.id === "custom" ? "Any OpenAI-compatible endpoint, e.g. http://localhost:8000/v1" : "Change only if you use a proxy or gateway."}</span>
      </div>
    </div>

    <div class="card stack">
      <div class="inline">
        <div style="flex:1">
          <h2>Test the connection</h2>
          <div class="muted">Sends a tiny request to check the key and model.</div>
        </div>
        <button class="btn btn-primary" data-test>Test connection</button>
      </div>
      ${testLine()}
    </div>`;
}

function timeAgo(ts: number): string {
  const s = Math.round((Date.now() - ts) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return new Date(ts).toLocaleDateString();
}

function renderHistory(): string {
  return `
    <h1>History</h1>
    <p class="lead">Your last 50 results, stored only on this computer. Click one to copy it.</p>
    <div class="card flush">
      <div class="row">
        <div class="text"><div class="title">Save history</div>
          <div class="desc">When off, nothing new is written to disk.</div></div>
        <div class="control">${toggle("historyEnabled", settings().historyEnabled)}</div>
      </div>
    </div>
    <div class="inline">
      <span class="caption" style="flex:1">${history.length} ${history.length === 1 ? "entry" : "entries"}</span>
      <button class="btn btn-sm btn-danger" data-clear-history ${history.length ? "" : "disabled"}>${icon("trash")}Clear all</button>
    </div>
    ${
      history.length
        ? history
            .map(
              (h, i) => `<button class="history-item" data-copy-history="${i}">
                <div class="meta"><span class="badge badge-soft">${esc(h.action)}</span><span>${timeAgo(h.ts)}</span></div>
                ${h.input ? `<div class="before clamp" dir="auto">${esc(h.input)}</div>` : ""}
                <div class="clamp" dir="auto">${esc(h.output)}</div>
              </button>`,
            )
            .join("")
        : `<div class="empty">Nothing yet. Select some text anywhere and press ${esc(accel("menu") || "your shortcut")}.</div>`
    }`;
}

function renderAbout(): string {
  return `
    <div class="about-hero">
      ${logo(64)}
      <div class="name">Clear</div>
      <div class="muted">Version ${esc(info.version)} · Select. Shortcut. Better text.</div>
      <button class="link-btn" data-open-repo title="github.com/Sulaiman-Dawood/clear">${icon("github")}View on GitHub</button>
    </div>
    <div class="caption section-title">Your data</div>
    <div class="card flush">
      <div class="row">
        <span class="row-icon">${icon("folder")}</span>
        <div class="text"><div class="title">Settings and history</div><div class="path">${esc(info.configDir)}</div></div>
        <div class="control"><button class="btn btn-sm btn-outline" data-open-dir>Open folder</button></div>
      </div>
      <div class="row">
        <span class="row-icon">${icon("key")}</span>
        <div class="text"><div class="title">API keys</div>
          <div class="desc">Windows Credential Manager, under <span class="mono">${esc(info.keychainService)}</span></div></div>
      </div>
      <div class="row">
        <span class="row-icon">${icon("globe")}</span>
        <div class="text"><div class="title">Network</div>
          <div class="desc">Clear only talks to the AI provider you pick. No accounts, no analytics.</div></div>
      </div>
    </div>
    <div class="card flush">
      <div class="row">
        <div class="text"><div class="title">Reset Clear</div>
          <div class="desc">Erases settings, history and every saved API key. This can't be undone.</div></div>
        <div class="control"><button class="btn btn-sm btn-danger" data-reset-all>${resetArmed ? "Click again to erase everything" : "Reset and erase"}</button></div>
      </div>
    </div>`;
}

// ---------- actions ----------

async function refreshHistory() {
  history = await api.getHistory();
  if (section === "history") render();
}

async function runTest() {
  testState = { status: "running", text: "" };
  render();
  try {
    const r = await api.testConnection();
    testState = { status: "ok", text: `Connected to ${r.model} in ${(r.ms / 1000).toFixed(1)} s` };
  } catch (e) {
    testState = { status: "error", text: errorMessage(e) };
  }
  render();
}

async function loadModels() {
  const p = activePreset();
  modelsLoading = true;
  modelsError = "";
  render();
  try {
    loadedModels[p.id] = await api.listModels(p.id, profile().baseUrl);
    if (!loadedModels[p.id].length) modelsError = "The server returned no models.";
  } catch (e) {
    modelsError = errorMessage(e);
  }
  modelsLoading = false;
  render();
}

// ---------- shortcut recorder ----------
//
// Records modifiers + a key (Ctrl+Shift+K) or a single key on its own
// (Insert, PageDown, F8). The app validates them; see chord.rs.

const MODIFIER_CODES = new Set([
  "ControlLeft", "ControlRight", "AltLeft", "AltRight", "ShiftLeft", "ShiftRight",
  "MetaLeft", "MetaRight", "OSLeft", "OSRight",
]);

let recSubmitting = false;

async function startRecording(action: string) {
  recording = action;
  recordError = "";
  await api.pauseHotkeys(true);
  render();
}

async function stopRecording() {
  recording = null;
  recordError = "";
  info.hotkeyErrors = await api.pauseHotkeys(false);
  render();
}

/** Ctrl / Alt / Shift / Win held during a key event, in the app's standard order. */
function modifierTokens(e: KeyboardEvent): string[] {
  return [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean) as string[];
}

function keyName(code: string): string | null {
  let m = code.match(/^Key([A-Z])$/);
  if (m) return m[1];
  m = code.match(/^Digit(\d)$/);
  if (m) return m[1];
  if (/^F([1-9]|1\d|2[0-4])$/.test(code)) return code;
  if (/^Numpad(\d|Add|Subtract|Multiply|Divide|Decimal)$/.test(code)) return code;
  const allowed = [
    "Space", "Enter", "Tab", "Backspace", "Escape", "Backquote", "Minus", "Equal", "BracketLeft", "BracketRight",
    "Backslash", "Semicolon", "Quote", "Comma", "Period", "Slash", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight",
    "Home", "End", "PageUp", "PageDown", "Insert", "Delete", "Pause", "ScrollLock", "PrintScreen", "ContextMenu",
  ];
  return allowed.includes(code) ? code : null;
}

/** Show the modifiers being held in the recording box, e.g. "Ctrl + Shift + …". */
function showLiveKeys(e: KeyboardEvent) {
  const box = document.querySelector<HTMLElement>(".hotkey-btn.recording");
  if (!box) return;
  const tokens = modifierTokens(e);
  box.innerHTML = tokens.length ? `${formatAccelerator(tokens.join("+"))}<span class="muted">+ …</span>` : "Press keys…";
}

async function submitShortcut(accelerator: string) {
  const action = recording;
  if (!action || recSubmitting) return;
  recSubmitting = true;
  try {
    const res = await api.setHotkey(action, accelerator);
    info.settings = res.settings;
    recording = null;
    recordError = "";
    info.hotkeyErrors = await api.pauseHotkeys(false);
    flash("Shortcut saved");
  } catch (err) {
    recordError = errorMessage(err);
  } finally {
    recSubmitting = false;
    render();
  }
}

function regularKey(e: KeyboardEvent) {
  const key = keyName(e.code);
  if (!key) {
    recordError = "That key can't be used in a shortcut.";
    return render();
  }
  void submitShortcut([...modifierTokens(e), key].join("+"));
}

window.addEventListener(
  "keydown",
  (e) => {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (MODIFIER_CODES.has(e.code)) return showLiveKeys(e);
    if (e.repeat) return;
    if (e.code === "Escape" && modifierTokens(e).length === 0) return void stopRecording();
    // Windows only sends Print Screen on release; handled in keyup.
    if (e.code === "PrintScreen") return;
    regularKey(e);
  },
  true,
);

window.addEventListener(
  "keyup",
  (e) => {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.code === "PrintScreen") return regularKey(e);
    if (MODIFIER_CODES.has(e.code)) showLiveKeys(e);
  },
  true,
);

void getCurrentWindow().onFocusChanged(({ payload: focused }) => {
  if (!focused && recording) void stopRecording();
  if (focused && section === "history") void refreshHistory();
});

document.addEventListener("click", async (e) => {
  const el = (e.target as HTMLElement).closest<HTMLElement>("button");
  if (!el) return;
  const d = el.dataset;

  if (d.nav) return go(d.nav as Section);
  if ("openMenu" in d) return void api.openMenu();
  if ("test" in d) return void runTest();
  if ("loadModels" in d) return void loadModels();
  if (d.record) return void (recording === d.record ? stopRecording() : startRecording(d.record));
  if (d.clearHotkey) {
    try {
      const res = await api.setHotkey(d.clearHotkey, "");
      info.settings = res.settings;
      info.hotkeyErrors = res.hotkeyErrors;
      render();
    } catch (err) {
      flash(errorMessage(err));
    }
    return;
  }
  if ("resetHotkeys" in d) {
    const res = await api.resetHotkeys();
    info.settings = res.settings;
    info.hotkeyErrors = res.hotkeyErrors;
    flash("Shortcuts reset");
    return render();
  }
  if (d.themeChoice) {
    settings().theme = d.themeChoice as Theme;
    applyTheme(settings().theme);
    save(0);
    return render();
  }
  if (d.mode && d.value) {
    settings().hotkeyModes[d.mode] = d.value as "replace" | "popup";
    save(0);
    return render();
  }
  if (d.effort) {
    settings().anthropicEffort = d.effort as Settings["anthropicEffort"];
    save(0);
    return render();
  }
  if ("saveKey" in d) {
    const input = $("#api-key") as HTMLInputElement;
    try {
      info.keyStatus = await api.setApiKey(activePreset().id, input.value);
      testState = { status: "idle", text: "" };
      flash("Key saved");
      render();
    } catch (err) {
      flash(errorMessage(err));
    }
    return;
  }
  if ("clearKey" in d) {
    info.keyStatus = await api.clearApiKey(activePreset().id);
    flash("Key removed");
    return render();
  }
  if ("addPreset" in d) {
    const label = ($("#preset-label") as HTMLInputElement).value.trim();
    const instruction = ($("#preset-instruction") as HTMLTextAreaElement).value.trim();
    if (!label || !instruction) return flash("Add a label and an instruction");
    settings().customPresets.push({ id: Date.now().toString(36), label, instruction });
    save(0);
    return render();
  }
  if (d.deletePreset) {
    settings().customPresets = settings().customPresets.filter((p) => p.id !== d.deletePreset);
    save(0);
    return render();
  }
  if ("clearHistory" in d) {
    await api.clearHistory();
    history = [];
    flash("History cleared");
    return render();
  }
  if (d.copyHistory) {
    const entry = history[Number(d.copyHistory)];
    if (entry) {
      await api.copyText(entry.output);
      flash("Copied to clipboard");
    }
    return;
  }
  if ("openDir" in d) return void api.openConfigDir();
  if ("openRepo" in d) return void api.openRepo();
  if ("resetAll" in d) {
    if (!resetArmed) {
      resetArmed = true;
      render();
      window.setTimeout(() => {
        resetArmed = false;
        if (section === "about") render();
      }, 4000);
      return;
    }
    resetArmed = false;
    info = await api.resetAll();
    history = [];
    flash("Clear was reset");
    return render();
  }
});

document.addEventListener("change", (e) => {
  const el = e.target as HTMLInputElement | HTMLSelectElement;
  const d = el.dataset;
  if (d.quick !== undefined) {
    settings().quickLanguages[Number(d.quick)] = el.value;
    return save();
  }
  if (d.toggle) {
    (settings() as unknown as Record<string, boolean>)[d.toggle] = (el as HTMLInputElement).checked;
    return save(0);
  }
  if (d.core) {
    const set = new Set(settings().disabledActions);
    if ((el as HTMLInputElement).checked) set.delete(d.core);
    else set.add(d.core);
    settings().disabledActions = [...set];
    return save(0);
  }
  if (d.extra) {
    const set = new Set(settings().enabledExtras);
    if ((el as HTMLInputElement).checked) set.add(d.extra);
    else set.delete(d.extra);
    settings().enabledExtras = [...set];
    return save(0);
  }
  if ("provider" in d) {
    settings().activeProvider = el.value;
    testState = { status: "idle", text: "" };
    modelsError = "";
    editingCustomModel = false;
    save(0);
    return render();
  }
  if ("addShortcut" in d) {
    if (el.value) void startRecording(el.value);
    return;
  }
  if ("model" in d) {
    if (el.value === OTHER_MODEL) {
      editingCustomModel = true;
      render();
      return $("#model-custom").focus();
    }
    editingCustomModel = false;
    setProfile("model", el.value);
    return render();
  }
  if (d.profile) {
    setProfile(d.profile as "model" | "baseUrl", el.value.trim());
    if (d.profile === "model") {
      editingCustomModel = false;
      return render();
    }
    // A new server URL means a new model list.
    const id = settings().activeProvider;
    delete loadedModels[id];
    autoLoaded.delete(id);
    return render();
  }
});

function setProfile(field: "model" | "baseUrl", value: string) {
  const id = settings().activeProvider;
  settings().providers[id] = { ...profile(), [field]: value };
  testState = { status: "idle", text: "" };
  save(0);
}

document.addEventListener("keydown", (e) => {
  const t = e.target as HTMLElement;
  if (e.key === "Enter" && t.id === "api-key") {
    e.preventDefault();
    document.querySelector<HTMLButtonElement>("[data-save-key]")?.click();
  }
  if (e.key === "Enter" && (t.id === "model-custom" || t.id === "base-url")) {
    e.preventDefault();
    (t as HTMLInputElement).blur();
  }
});

// ---------- start ----------

let pendingSection: Section | null = null;

async function init() {
  info = await api.getAppInfo();
  if (pendingSection) section = pendingSection;
  else if (needsSetup()) section = "provider";
  render();
}

void listen<string>("navigate", (e) => {
  if (!SECTIONS.some((s) => s.id === e.payload)) return;
  if (info) go(e.payload as Section);
  else pendingSection = e.payload as Section;
});

void init();
