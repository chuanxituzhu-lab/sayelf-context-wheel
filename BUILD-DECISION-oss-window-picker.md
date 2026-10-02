# Build Decision Record — OSS Distillation: Running-App Picker

Date: 2026-10-02

## Decision

- **Real task:** Review comparable GitHub radial-menu tools and integrate a worthwhile usability improvement into Context Wheel.
- **Closest solutions:** StarPie supports application-specific profiles and capturing running windows; GoPieMenu exposes a smart window picker for binding profiles; AutoHotPie documents alternate interaction modes for mice/controllers that do not reliably send release events. Context Wheel already has per-app and per-mode profiles, hold/move/release selection, multiple rings, native AutoCAD icons, editable actions, and local habit export.
- **Step 0 classification / verdict:** `Improve` / `GO`. The profile editor currently asks users to type both an application name and executable name. A local running-window picker removes manual executable entry and reduces profile-binding errors without duplicating the existing wheel.
- **Measurable difference:** Create an application profile by choosing a visible running app; persist its executable name only. Keep the manual executable entry fallback. No window title is stored or logged.
- **Success evidence:** A selected window yields its correct executable in the draft profile; duplicate executable profiles are rejected; blank/system/Context Wheel windows are excluded; existing manual profile creation, profile validation, and focused build/tests continue to pass.
- **Minimum Core:** One on-demand Win32 visible-window enumerator, one small picker dialog, and reuse of the existing profile creation/validation path.
- **Plugin boundary:** Enumeration is a Windows platform adapter; profile creation stays in the existing profile editor. Other operating systems are out of scope.
- **Local-first boundary:** Window titles and executable paths are read only after the user opens the picker, shown locally, and discarded after selection. Only the selected executable and user-entered profile name enter the unsaved profile draft.
- **Data classification / transfer:** GitHub documentation is `Public`; running window titles and executable paths are local `Sensitive` context. No external transfer, telemetry, or title logging.
- **GitHub/public release:** No remote connection or public release in this task. The code remains in the local working tree; any later public release requires a separate staged-diff and artifact review.
- **State/check rule:** Enumerate only on explicit picker open/refresh; do not poll. Continue only with visible titled windows whose executable can be read.
- **Epistemic labels:** `Fact` — current app-profile creation prompts for the app and executable separately; StarPie and GoPieMenu document app-specific profiles and running-window selection. `Inference` — selection should reduce executable-name entry errors. `Unknown` — behavior with elevated/protected windows varies by Windows permissions; inaccessible processes will be skipped.
- **Evolution / rollback:** The dialog is additive and manual entry remains available. Removing the picker command/dialog restores the prior profile creation path; persisted profile schema is unchanged.
- **WebUI decision:** Required; this capability belongs in the existing Profile Studio and does not need a separate page.
- **Simplest implementation:** Reuse existing Tauri and Win32 dependencies; no new runtime dependency, service, icon library, or network call.
- **Explicitly not built:** Importing competitor code/assets, OCR/AI application detection, automatic background monitoring, global release-fallback modes, cloud sync, telemetry, or changes to wheel geometry/action semantics.

## Distilled references

- [StarPie](https://github.com/SoftBlack42/StarPie): per-app profiles, window capture, hold-to-select, and preserving ordinary mouse use; MIT license.
- [GoPieMenu](https://github.com/RyuuMeow/GoPieMenu): per-app filters, a smart running-window picker, icon search, and JSON import/export; GPL-3.0, so only the interaction idea is reused.
- [AutoHotPie](https://github.com/dumbeau/AutoHotPie): documents alternate activation for devices that do not reliably signal button release; useful for future compatibility tests, not needed for this user's currently working side-button trigger.
- [Radify](https://github.com/JoyHak/Radify): demonstrates multi-ring menus and loading icons from local EXE/DLL resources; Context Wheel already implements multi-ring CAD menus and a tested local AutoCAD CUIx/XMX provider.

No source code, icons, assets, or copied configuration from these projects are included.

## Publication addendum — 2026-10-02

The user later explicitly requested upload to the existing public repository chuanxituzhu-lab/sayelf-context-wheel. For that bounded transfer, project source and the product logo/assets are classified Public. Window titles, executable paths, profiles, habit data, logs, and local test evidence remain local and are excluded. No competitor code or assets are included.
