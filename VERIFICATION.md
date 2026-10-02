# v0.1.4 Verification

Date: 2026-10-02

## Research decision

`Improve`: similar tools already establish per-application radial menus and hold/move/release interaction. Context Wheel already had those features, multi-ring CAD profiles, local native AutoCAD icons, and habit transfer. The verified gap was manual entry of both an application name and its executable when creating a new app profile. v0.1.4 adds an on-demand running-window picker and retains manual entry.

No competitor code, icons, assets, or configuration were copied. Window titles are only shown in the local picker; the profile stores the executable name and editable profile name.

## Checks

- `npm ci --no-audit --no-fund` — passed.
- `npm run build` — passed; TypeScript check and Vite production build completed.
- `node tests/schema.mjs` — passed; schema, 124 commands, and 95 icons validated.
- `node tests/wheel-model.test.mjs` — passed; 5 themes, 16-slot fill/clear/resize, and icon matching validated.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` — passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --locked` — passed, 30 tests.
- `npm run tauri -- build --bundles nsis` — passed; produced the x64 v0.1.4 installer.
- Source archive inspection — 46 entries; contains Rust and frontend source plus the decision record; excludes `node_modules`, `dist`, and `target`.

## Artifacts

- Installer: `Context Wheel_0.1.4_x64-setup.exe` — 4,649,965 bytes; SHA-256 `E202AA99FFAF8C8F98674CC6F305AC992795F29985D214A6C64B986F684CBA44`.
- Source: `sayelf-context-wheel-source-v0.1.4-final.zip` — 2,528,210 bytes; SHA-256 `80B83DF51E4A8552958AA45053CF8DD36719A817DFD14A771C02F3F28CFBB704`.
- The installer is unsigned; Windows may show an unknown-publisher warning.

## Limits

- The desktop E2E suite was not run because it injects mouse movement and temporarily takes control of the cursor. Real CAD commands were not revalidated in this task.
- The running-window picker was compiled and its candidate de-duplication/filtering was unit-tested, but its dialog was not manually exercised in the desktop UI. Windows windows whose process details cannot be queried are skipped.
- The v0.1.4 current-user installer was successfully installed on Windows and the application launched. The installer is unsigned; Windows may show an unknown publisher warning.
