// Dev-only: fakes the Rust backend so the pages can be previewed in a normal
// browser (`npm run dev`, then open /popup.html or /settings.html).
// Never included in release builds; see the guard in boot.ts.
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import type { AppInfo, GenEvent, OpenPayload, Settings, ToastPayload } from "./ipc";

const settings: Settings = {
  activeProvider: "anthropic",
  providers: {
    anthropic: { baseUrl: "https://api.anthropic.com", model: "claude-opus-5-5" },
    ollama: { baseUrl: "http://localhost:11434/v1", model: "" },
  },
  anthropicEffort: "low",
  hotkeys: { menu: "Ctrl+Shift+Space", fix: "Ctrl+Alt+Space", translate: "Ctrl+Alt+T", summarize: "Ctrl+Alt+S" },
  quickLanguages: ["English", "Arabic", "Spanish"],
  enabledExtras: ["friendly", "bullets"],
  disabledActions: [],
  hotkeyModes: {},
  customPresets: [],
  historyEnabled: false,
  launchAtLogin: false,
  theme: "system",
};

const SAMPLE = "hey team, i think we maybe should of shipped the the update yesterday but their was a bug in the login flow.";
const FIXED = "Hey team, I think we should have shipped the update yesterday, but there was a bug in the login flow.";

export const sampleOpen: OpenPayload = {
  id: 1,
  hasSelection: true,
  canReplace: true,
  editable: true,
  autorun: null,
  ready: null,
  preview: SAMPLE,
  text: SAMPLE,
  words: SAMPLE.split(/\s+/).length,
  chars: SAMPLE.length,
  actions: [
    { id: "fix", label: "Fix", description: "", icon: "pen", group: "core", kind: "transform", diffFirst: true },
    { id: "clearer", label: "Clearer", description: "", icon: "sparkles", group: "core", kind: "transform", diffFirst: false },
    { id: "shorter", label: "Shorter", description: "", icon: "scissors", group: "core", kind: "transform", diffFirst: false },
    { id: "formal", label: "Formal", description: "", icon: "briefcase", group: "core", kind: "transform", diffFirst: false },
    { id: "format", label: "Format", description: "", icon: "hash", group: "core", kind: "transform", diffFirst: true },
    { id: "summarize", label: "Summary", description: "", icon: "align", group: "core", kind: "answer", diffFirst: false },
    { id: "explain", label: "Explain", description: "", icon: "help", group: "core", kind: "answer", diffFirst: false },
    { id: "friendly", label: "Friendlier", description: "", icon: "smile", group: "tone", kind: "transform", diffFirst: false },
    { id: "bullets", label: "Bullet list", description: "", icon: "list", group: "structure", kind: "transform", diffFirst: false },
  ],
  quickLanguages: ["English", "Arabic", "Spanish"],
  providerName: "Anthropic (Claude)",
  model: "claude-opus-5-5",
  problem: null,
};

let genId = 100;

/** What the fake AI writes; demo scenes (demo.ts) can change it. */
export const fakeAi = { output: FIXED };

async function streamFake(id: number) {
  const output = fakeAi.output;
  const words = output.split(/(\s+)/);
  for (const w of words) {
    await new Promise((r) => setTimeout(r, 25));
    await emit<GenEvent>("gen", { id, kind: "delta", text: w });
  }
  await emit<GenEvent>("gen", { id, kind: "done", text: output });
}

export function install(page: "popup" | "settings") {
  // Screenshot scenes: ?demo=menu|fix|rtl|toast (popup), ?section=... (settings),
  // plus &theme=light|dark and &bg=light|dark|none. See demo.ts.
  const params = new URLSearchParams(location.search);
  const theme = params.get("theme");
  if (theme === "light" || theme === "dark") settings.theme = theme;
  mockWindows(page);
  mockIPC(
    (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      switch (cmd) {
        case "popup_sync":
          return null;
        case "get_theme":
          return settings.theme;
        case "popup_begin_resize":
          return a.height;
        case "start_generation": {
          const id = ++genId;
          setTimeout(() => void streamFake(id), 50);
          const action = String(a.action);
          const known = sampleOpen.actions.find((x) => x.id === action);
          const label = action === "translate" ? `Translate · ${a.language ?? "English"}` : (known?.label ?? "Custom");
          return { id, label, kind: known?.kind ?? "transform", diffFirst: known?.diffFirst ?? false };
        }
        case "get_app_info":
          return {
            settings,
            keyStatus: { anthropic: true },
            hotkeyErrors: {},
            providers: [
              { id: "anthropic", name: "Anthropic (Claude)", wire: "anthropic", baseUrl: "https://api.anthropic.com", defaultModel: "claude-opus-5-5", needsKey: true, local: false, suggestedModels: ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"], keyHint: "Create a key at platform.claude.com" },
              { id: "ollama", name: "Ollama (local)", wire: "openAi", baseUrl: "http://localhost:11434/v1", defaultModel: "", needsKey: false, local: true, suggestedModels: [], keyHint: "Runs on this computer" },
            ],
            catalog: sampleOpen.actions,
            version: "1.1.0",
            configDir: "C:\\Users\\you\\AppData\\Roaming\\app.clear.writer",
            keychainService: "app.clear.writer",
          } satisfies AppInfo;
        case "save_settings":
          return a.settings;
        case "set_hotkey":
          // The real app validates and normalizes; the preview just stores it.
          settings.hotkeys[String(a.action)] = String(a.accelerator);
          (window as unknown as { __lastHotkey: string }).__lastHotkey = String(a.accelerator);
          return { settings, hotkeyErrors: {} };
        case "pause_hotkeys":
          return {};
        case "list_models":
          return new Promise((r) =>
            setTimeout(() => r(["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5", "claude-opus-4-8"]), 300),
          );
        case "get_history":
          return [{ ts: Date.now() - 120000, action: "Fix", input: SAMPLE, output: FIXED }];
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );

  // Drive popup states from the console: __clear.open(), __clear.toast("working", "Fix…")
  Object.assign(window, {
    __clear: {
      open: (over: Partial<OpenPayload> = {}) => emit("popup-open", { ...sampleOpen, ...over, id: Date.now() }),
      toast: (state: ToastPayload["state"], text: string) => emit("toast", { id: Date.now(), state, text }),
    },
  });

  if (params.has("demo") || params.has("section")) {
    void import("./demo").then((m) => m.run(page, params));
  }
}
