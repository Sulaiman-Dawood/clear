// Import first in each page. In `vite` dev outside Tauri, installs a fake backend.
// `import.meta.env.DEV` is false in release builds, so the mock is dropped entirely.
if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  const page = location.pathname.includes("settings") ? "settings" : "popup";
  const { install } = await import("./devmock");
  install(page);
}

export {};
