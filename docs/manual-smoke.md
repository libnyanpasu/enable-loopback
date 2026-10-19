# Morning GUI smoke

Run from a **non-administrator PowerShell** on an **unlocked** desktop:

```powershell
Set-Location 'G:\Programs\Rust\enable-loopback'
$env:__COMPAT_LAYER = 'RunAsInvoker'
Start-Process -FilePath '.\target\debug\enableLoopback.exe' -ArgumentList '--lang en'
```

Use `--lang zh-cn`, `zh-tw`, `ja`, `ko`, or `ru` to check the other translations.
Do not invoke Save during this smoke: saving replaces the firewall exemption table.
Undo checkbox changes before a clean close, or choose Exit in the discard dialog.

- [ ] The initial window is 960×600 DIP, has the embedded icon, Mica/theme colors, explanation bar and Learn more link. Resize down to the 640×400 minimum. Toolbar, list and status remain usable.
- [ ] Seven headers and row columns align, including after horizontal scrolling. Long values are single-line ellipses; hovering a cell shows its full text. The file column receives spare width in a large window.
- [ ] Scroll through the list and check that rows remain responsive. The row count in the counter stays constant; only visible/nearby WinUI containers are realized.
- [ ] Click a row's text: only that row is selected. Ctrl+click another row adds/removes it. Shift+click selects the inclusive range from the anchor, including reverse ranges. Checkbox clicks toggle exemption without changing the selection.
- [ ] Selection remains visible while another row has WinUI's keyboard focus. Check readability in light, dark and Windows high-contrast themes. Current-user rows use a theme card fill and thin stroke; selected rows use a card fill and a thicker accent edge. Text uses the theme primary text brush.
- [ ] With focus in the list, Shift+Up/Down extends and shrinks the range; plain Up/Down moves to one row. Move across the viewport edge and confirm the focused row scrolls into view. Space toggles the focused row's exemption; Space on a focused checkbox uses its normal behavior.
- [ ] Ctrl+A selects all rows while focus is in the list. Ctrl+C copies selected rows in display order, with a machine-readable LoopbackExempt/LoopbackBlocked prefix and seven tab-separated fields. A single row has no trailing newline; multiple rows end with CRLF.
- [ ] Right-click a text cell in an already-selected row: the selection is preserved. Right-click an unselected row: it becomes the only selection. The popup contains Copy selected rows, Copy selected column, a separator, Exempt none and Exempt all. Copy selected column uses exactly the clicked column, including after horizontal scrolling, and ends each copied value with CRLF.
- [ ] Select a row near the bottom, then click each header. Ordinal sorting changes the order; clicking the same header reverses it. Selection stays attached to SIDs and the first selected row is brought into view.
- [ ] F5 and Alt+R refresh. While refreshing, the list and toolbar are disabled and the status bar shows a small progress ring. The completion time uses the current Windows user's long time format. Focus returns to the list.
- [ ] Toggle one checkbox: Save enables and the counter changes immediately. Toggle back: Save disables and the status clears. Exempt all/none update every checkbox and show their respective pending-change status when dirty.
- [ ] With dirty edits, F5/Refresh opens the discard dialog. Cancel is the default: Enter/Escape must preserve edits unless Refresh is explicitly chosen. Confirming Refresh discards edits and reloads the configuration.
- [ ] Ctrl+S and Alt+S trigger Save only when enabled. **Check this shortcut with Save disabled during the smoke.** Never enable it merely to test the shortcut on the real firewall. Successful writes and access-denied behavior are covered by model/dispatch tests; any live write requires a separately authorized configuration test.
- [ ] F1 and Alt+L open the Microsoft Learn loopback article. Clicking Learn more opens the same article. Hovering the link shows `enable-loopback v0.1.0 64bit Windows …` (or `arm64`); leaving restores the previous/current status.
- [ ] Alt+A and Alt+N invoke Exempt all/none. The reactor revision has no AccessKey property, so there are no Alt key-tip overlays. Revert edits with Refresh before finishing.
- [ ] With a dirty checkbox, use the title-bar ×. The window stays open and shows Exit without saving? with Cancel as the default. Cancel preserves the edits and keeps Save enabled. Repeat using Alt+F4, then Cancel again.
- [ ] Toggle back to clean state; both × and Alt+F4 now close without confirmation. Reopen, make a dirty edit and explicitly choose Exit to confirm the discard path.
- [ ] Launch with `manual --lang en`: list initially empty/disabled, Refresh available. Refresh populates it. Launch with `-registry --lang en`: it reads registry mappings and uses `?` for unavailable description/package data.
- [ ] If real orphan exemptions already exist, each SID appears as selectable text in configuration order. Unchecking Keep prompting suppresses subsequent orphan dialogs for the session. Do not create orphans for this smoke or save away existing ones.

The locked-desktop run verifies UIA item enumeration, Toggle/Selection/Invoke behavior,
sort restoration with an offscreen selection,
Save enablement, counts, refresh/discard and native WM_CLOSE confirmation. Real modifiers,
keyboard routing, popup position/column capture, hover, native close entry paths and visual
appearance require this checklist. Firewall writes are excluded from the UIA run.

Run the repeatable, read-only UIA smoke from PowerShell with `& .\scripts\gui-smoke.ps1`. It checks the child process is unelevated and compares the exemption table before/after. Its screenshot attempt may capture the lock screen instead of the app.
