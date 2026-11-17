// Dev-only screenshot scenes for the README and release images.
// Loaded by devmock.ts when the preview URL has ?demo=… or ?section=…
//
//   /popup.html?demo=menu&bg=light          the action menu
//   /popup.html?demo=fix&bg=light           a finished Fix, Changes view
//   /popup.html?demo=rtl&theme=dark&bg=dark English translated to Arabic
//   /popup.html?demo=toast&bg=light         the "working" toast
//   /settings.html?section=shortcuts        any Settings page
import { emit } from "@tauri-apps/api/event";
import { fakeAi, sampleOpen } from "./devmock";

const ENGLISH = "Hey team, I think we should have shipped the update yesterday, but there was a bug in the login flow.";
const ARABIC = "مرحبًا يا فريق، أعتقد أنه كان علينا إطلاق التحديث بالأمس، لكن كان هناك خلل في عملية تسجيل الدخول.";

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

function click(selector: string) {
  document.querySelector<HTMLElement>(selector)?.click();
}

/** Center the popup on a plain backdrop, for screenshots. */
function stage(bg: string) {
  const colors: Record<string, string> = { light: "#ececec", dark: "#1c1c1c", none: "transparent" };
  const style = document.createElement("style");
  style.textContent = `
    html, body.popup-body { background: ${colors[bg] ?? bg} !important; }
    body.popup-body { min-height: 100vh; display: grid; place-items: center; }
    .popup-root { padding: 24px; }`;
  document.head.appendChild(style);
}

export async function run(page: "popup" | "settings", params: URLSearchParams) {
  await wait(250);
  if (page === "settings") {
    const section = params.get("section");
    if (section) click(`[data-nav="${section}"]`);
    return;
  }

  const bg = params.get("bg");
  if (bg) stage(bg);
  const open = (over = {}) => emit("popup-open", { ...sampleOpen, ...over, id: Date.now() });

  switch (params.get("demo")) {
    case "menu":
      await open();
      break;
    case "fix":
      await open();
      await wait(150);
      click('[data-action="fix"]');
      break;
    case "rtl":
      fakeAi.output = ARABIC;
      await open({
        text: ENGLISH,
        preview: ENGLISH,
        words: ENGLISH.split(/\s+/).length,
        chars: ENGLISH.length,
        autorun: { action: "translate", language: "Arabic" },
      });
      break;
    case "toast":
      await emit("toast", { id: Date.now(), state: "working", text: "Fix…" });
      break;
  }
  // No focus rings or text cursors in screenshots.
  await wait(2600);
  (document.activeElement as HTMLElement | null)?.blur();
}
