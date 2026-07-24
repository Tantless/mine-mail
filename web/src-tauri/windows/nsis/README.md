# Mine Mail Windows NSIS theme

This directory customizes only the Tauri NSIS `setup.exe`. MSI, macOS and
Linux packages keep their platform defaults.

The implementation deliberately extends Tauri's maintained NSIS template via
`hooks.nsh` instead of replacing the whole template. This preserves Tauri's
upgrade detection, WebView2 bootstrap, shortcut creation, uninstaller and
silent-install behavior.

## Assets

- `assets/sidebar.bmp`: 164 × 314, 24-bit BMP for welcome and finish pages.
- `assets/header.bmp`: 150 × 57, 24-bit BMP for directory, progress and
  uninstaller pages.
- `assets/blank-window.ico`: transparent runtime icon that keeps the native
  title bar text-only without changing the branded `setup.exe` file icon.
- `../../icons/icon.ico`: installer and uninstaller executable icon.

The two BMP files were generated for the approved cute frosted Mine Mail
direction and contain no UI text. All Chinese copy remains native NSIS text so
it stays sharp and accessible at different display scales.

## Local verification

From `web`:

```powershell
npm run tauri build -- --bundles nsis
```

The result is written to `src-tauri/target/release/bundle/nsis/`.
Silent installation remains available through the uppercase `/S` argument.

This theme does not sign the installer. Public releases should use a trusted
Windows Authenticode certificate; otherwise SmartScreen or Smart App Control
can warn about or block a newly generated installer.
