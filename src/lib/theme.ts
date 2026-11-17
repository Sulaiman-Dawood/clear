// Light / dark / follow-Windows theme for both windows.
// The colors live in styles/tokens.css; this only sets data-theme on <html>.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Theme = "system" | "light" | "dark";

export function applyTheme(theme: Theme): void {
  if (theme === "light" || theme === "dark") document.documentElement.dataset.theme = theme;
  else delete document.documentElement.dataset.theme;
  try {
    localStorage.setItem("theme", theme);
  } catch {
    // Not remembered; the app sends the theme on load anyway.
  }
}

/** Apply the last known theme right away (no flash), then the saved one, and follow changes. */
export function initTheme(): void {
  try {
    const cached = localStorage.getItem("theme");
    if (cached === "light" || cached === "dark" || cached === "system") applyTheme(cached);
  } catch {
    // Storage unavailable: fall through to the app's value.
  }
  invoke<Theme>("get_theme")
    .then(applyTheme)
    .catch(() => {});
  void listen<Theme>("theme-changed", (e) => applyTheme(e.payload));
}
