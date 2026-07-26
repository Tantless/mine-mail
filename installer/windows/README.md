# Mine Mail Windows installer

This project is the user-visible Windows `setup.exe`.

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
npm install
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
  -PayloadPath ".\web\src-tauri\target\release\bundle\nsis\Mine Mail_0.1.4_x64-setup.exe" `
  -Version "0.1.4"
```

The resulting public asset is written to
`installer/windows/release-assets/`. Release builds also upload the maintained
Tauri NSIS installer and its updater signature. The branded setup remains the
user-facing first-install experience; the signed NSIS asset is used for
in-place updates initiated by the running app.
