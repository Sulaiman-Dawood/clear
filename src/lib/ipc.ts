// Typed wrappers around the Rust commands in src-tauri/src/commands.rs.
import { invoke } from "@tauri-apps/api/core";

export type Kind = "transform" | "answer";

export interface ActionInfo {
  id: string;
  label: string;
  description: string;
  icon: string;
  group: string;
  kind: Kind;
  diffFirst: boolean;
}

export interface OpenPayload {
  id: number;
  hasSelection: boolean;
  canReplace: boolean;
  editable: boolean;
  autorun: { action: string; language: string | null } | null;
  /** A finished result to show right away, with a note saying why it wasn't pasted. */
  ready: { action: string; label: string; kind: Kind; diffFirst: boolean; text: string; note: string } | null;
  preview: string;
  text: string;
  words: number;
  chars: number;
  actions: ActionInfo[];
  quickLanguages: string[];
  providerName: string;
  model: string;
  problem: string | null;
}

export interface ToastPayload {
  id: number;
  state: "working" | "done" | "error";
  text: string;
}

export interface GenEvent {
  id: number;
  kind: "delta" | "done" | "error";
  text: string;
}

export interface GenStart {
  id: number;
  label: string;
  kind: Kind;
  diffFirst: boolean;
}

export interface Preset {
  id: string;
  label: string;
  instruction: string;
}

export interface ProviderProfile {
  baseUrl: string;
  model: string;
}

export interface Settings {
  activeProvider: string;
  providers: Record<string, ProviderProfile>;
  anthropicEffort: "low" | "medium" | "high";
  hotkeys: Record<string, string>;
  quickLanguages: string[];
  enabledExtras: string[];
  disabledActions: string[];
  hotkeyModes: Record<string, "replace" | "popup">;
  customPresets: Preset[];
  historyEnabled: boolean;
  launchAtLogin: boolean;
  theme: "system" | "light" | "dark";
}

export interface ProviderPreset {
  id: string;
  name: string;
  wire: "anthropic" | "openAi";
  baseUrl: string;
  defaultModel: string;
  needsKey: boolean;
  local: boolean;
  suggestedModels: string[];
  keyHint: string;
}

export interface AppInfo {
  settings: Settings;
  keyStatus: Record<string, boolean>;
  hotkeyErrors: Record<string, string>;
  providers: ProviderPreset[];
  catalog: ActionInfo[];
  version: string;
  configDir: string;
  keychainService: string;
}

export interface HotkeyUpdate {
  settings: Settings;
  hotkeyErrors: Record<string, string>;
}

export interface HistoryEntry {
  ts: number;
  action: string;
  input: string;
  output: string;
}

export interface TestResult {
  model: string;
  ms: number;
  reply: string;
}

export const api = {
  popupSync: () => invoke<OpenPayload | null>("popup_sync"),
  popupPresent: (id: number, width: number, height: number, focus: boolean, placement: "menu" | "toast") =>
    invoke<void>("popup_present", { id, width, height, focus, placement }),
  popupResize: (width: number, height: number) => invoke<void>("popup_resize", { width, height }),
  popupHide: () => invoke<void>("popup_hide"),
  cancelDirect: () => invoke<void>("cancel_direct"),
  popupDrag: () => invoke<void>("popup_drag"),
  popupBeginResize: (width: number, height: number) => invoke<number>("popup_begin_resize", { width, height }),
  startGeneration: (action: string, instruction?: string, language?: string) =>
    invoke<GenStart>("start_generation", { action, instruction, language }),
  cancelGeneration: () => invoke<void>("cancel_generation"),
  applyResult: (text: string, mode: "replace" | "copy") => invoke<void>("apply_result", { text, mode }),

  getAppInfo: () => invoke<AppInfo>("get_app_info"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  setApiKey: (provider: string, key: string) => invoke<Record<string, boolean>>("set_api_key", { provider, key }),
  clearApiKey: (provider: string) => invoke<Record<string, boolean>>("clear_api_key", { provider }),
  testConnection: () => invoke<TestResult>("test_connection"),
  listModels: (provider: string, baseUrl: string) => invoke<string[]>("list_models", { provider, baseUrl }),
  setHotkey: (action: string, accelerator: string) => invoke<HotkeyUpdate>("set_hotkey", { action, accelerator }),
  resetHotkeys: () => invoke<HotkeyUpdate>("reset_hotkeys"),
  pauseHotkeys: (paused: boolean) => invoke<Record<string, string>>("pause_hotkeys", { paused }),
  getHistory: () => invoke<HistoryEntry[]>("get_history"),
  clearHistory: () => invoke<void>("clear_history"),
  copyText: (text: string) => invoke<void>("copy_text", { text }),
  openConfigDir: () => invoke<void>("open_config_dir"),
  openRepo: () => invoke<void>("open_repo"),
  resetAll: () => invoke<AppInfo>("reset_all"),
  openMenu: () => invoke<void>("open_menu_from_settings"),
  showSettings: (section?: string) => invoke<void>("show_settings", { section }),
};

export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
