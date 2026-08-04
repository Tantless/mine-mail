# Mine Mail Windows installer

This project is the user-visible Windows 11 x64 `setup.exe` for AMD and Intel
64-bit processors.

- React owns the frameless branded interface.
- Rust owns window controls, path validation, payload extraction and lifecycle
  state.
- Tauri's NSIS installer remains embedded as a silent payload so file copying,
  shortcuts and Windows uninstall registration stay on the maintained path.
- The completion screen can add or remove the real desktop shortcut and can
  enable the same `Mine Mail --background` startup entry used by the app's
  settings page. Desktop shortcut defaults to the installed state; autostart
  remains off for a fresh install.

The release build intentionally fails when `MINE_MAIL_NSIS_PAYLOAD` is missing.
This prevents publishing an attractive installer that cannot install the app.

## Local frontend preview

```powershell
cd installer/windows
npm ci
npm run dev
```

The browser preview simulates the three visible states. It never writes files.

## Build a real setup executable

First build Mine Mail's internal NSIS payload:

```powershell
cd web
npm run tauri:build -- --bundles nsis
```

Then build the branded shell:

```powershell
cd ..
.\installer\windows\scripts\build-release.ps1 `
  -PayloadPath ".\web\src-tauri\target\release\bundle\nsis\Mine Mail_<version>_x64-setup.exe" `
  -Version "<version>"
```

Replace `<version>` with the version in `web/src-tauri/tauri.conf.json`.
The resulting public asset is written to
`installer/windows/release-assets/`. The branded setup accepts Tauri's passive
and quiet updater arguments, then delegates them to its embedded maintained NSIS
payload. Release builds sign this outer setup and point Windows updater metadata
to it, so the public Release exposes only one Windows executable. The internal
NSIS payload and the temporary outer signature are removed before publication.
