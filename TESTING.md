# Testing Clear

Automated tests cover prompts, shortcut rules and AI streaming:

```bash
cd src-tauri
cargo test --lib
```

The clipboard round-trip test uses your real clipboard (and puts it back), so it only runs on request:

```bash
cargo test --lib -- --ignored clipboard
```

Copying, pasting and shortcuts in other apps can only be checked by hand. Run through this list
before each release. It takes about 10 minutes. Use a text box (Notepad), a browser page, and
one Electron app (Slack, Discord or VS Code).

## Popup

- [ ] Select text in Notepad and press the menu shortcut. The popup opens next to the text and shows the word count.
- [ ] Run **Fix** and click **Replace**. The corrected text replaces the selection, and what you had copied before is still on the clipboard.
- [ ] Copy a **file** in Explorer, then do the step above. Pasting in Explorer still pastes the file.
- [ ] Copy **formatted text** from Word or a web page, then do the step above. Pasting in Word keeps the formatting.
- [ ] Press the menu shortcut with nothing selected. The popup says nothing is selected; ask a question and **Insert** it.
- [ ] Select text on a web page (not in a text box) and run Fix. **Copy** is the main button.
- [ ] Translate Arabic or Hebrew text: the result is right-to-left, and the grip sits on the scrollbar side.
- [ ] Drag the popup by its header, drag the output grip, click the gear (Settings opens), press Esc (it closes).
- [ ] Click into another window while the popup is open. The popup closes.

## In-place shortcuts (e.g. Fix set to "In place")

- [ ] Select text in Notepad and press the shortcut. "Fix…" shows at the bottom, then the text is replaced, and "Done" fades.
- [ ] Press the shortcut, then press it again while "Fix…" shows. It cancels and nothing is pasted.
- [ ] Press the shortcut, then click the "Fix…" toast. It cancels.
- [ ] Press the shortcut, then switch to another app before it finishes. Nothing is pasted, and the result opens in the popup with a note.
- [ ] Press the shortcut, then click elsewhere in the same text before it finishes. Same as above.
- [ ] Press the shortcut on a web page's text (not editable). The result opens in the popup instead.

## Shortcuts

- [ ] Add a shortcut for any action with **Add a shortcut…**, using a combination (Ctrl+Alt+K) and a single key (Page Down).
- [ ] Try one that another app already owns. Clear says so on that row.
- [ ] Try a lone letter. Clear refuses it.

## Settings

- [ ] Switch **Appearance** between System, Light and Dark. Both windows follow, including the title bar.
- [ ] **Test connection** works with your key; a wrong key shows a clear error.
- [ ] With **History** off, nothing new appears in History after using Clear.

## Installer

- [ ] Quit the dev version (tray icon > Quit), install `Clear_x.y.z_x64-setup.exe`, and check that it starts from the Start menu.
- [ ] Turn on **Launch at login**, sign out and back in. Clear is in the tray and Settings doesn't open by itself.
