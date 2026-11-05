# Native DOMINION desktop

The native app uses Tauri 2 with WKWebView on macOS 13.3 or newer and WebView2 on Windows. Both run the same DOMINION Rust engine and the React/Vite interface. Node and npm are build tools; neither is needed after installation. Other agent projects and engines are not bundled.

## Build

Install Rust stable, the platform's native build tools, and Node 24 or newer. On Windows use MSVC with the C++ desktop workload. Then:

```sh
npm ci --prefix web
node scripts/build-desktop.mjs
```

macOS produces `src-tauri/target/release/bundle/macos/DOMINION.app` and a DMG under `bundle/dmg/`. Windows builds `.msi` and NSIS `-setup.exe` installers under `bundle/msi/` and `bundle/nsis/`. The GitHub workflow builds Apple Silicon, Intel macOS, and Windows MSVC independently. A macOS build does not prove that Windows has been built; download Windows artifacts only after that workflow succeeds.

Use `--target aarch64-apple-darwin`, `--target x86_64-apple-darwin`, or `--target x86_64-pc-windows-msvc` on the corresponding supported build host. `--bundles app,dmg` and `--bundles nsis,msi` select installers. Rust engine artifacts use `target/desktop-engine`; the host uses `src-tauri/target`, independently of normal engine tests.

Unsigned local packages are suitable for local testing. Public distribution should configure the developer's Apple signing/notarization or Windows signing credentials; no certificate or private key belongs in this repository. The workflow creates draft releases for version tags and never claims signing when credentials are absent.

## Runtime and data

The host first checks whether the existing `127.0.0.1:4317` endpoint identifies itself as DOMINION's Rust engine. If so it displays that engine and never owns, stops, or replaces the existing service. Otherwise it starts only its bundled Rust sidecar, selects an available loopback port, and verifies the unique instance ID returned by health. The remembered port is reused when free so browser preferences retain a stable origin.

A standalone app stores its SQLite database, encryption key, logs, and writable task workspace under Tauri's OS app-data directory for `com.hushmind.dominion.desktop`. On macOS this is normally `~/Library/Application Support/com.hushmind.dominion.desktop`; on Windows it is the user's app-data directory for that identifier. Nothing is written into the installed application bundle. Existing development-service data stays in its original directory when the host attaches to that service.

The app contains only the own compiled Rust engine, compiled interface, static channel manifests, and icons. It does not include the developer's repository location, private `runtime/`, keys, sessions, profiles, or `node_modules`. No external browser is used to display the main application.

Quit closes the owned sidecar's input pipe, allowing the Rust backend to stop and release its tool tasks. The host kills only its own child if the graceful shutdown exceeds two seconds. Reopening the app reuses durable records and the engine's existing recovery gates. On macOS closing the window hides it; the app's Quit command stops the app. The system's app switcher or Dock reopens the hidden window.

## Native menus

The Edit menu and right-click popup use native Cut, Copy, Paste and Select All actions. The popup also has Back, Forward and Reload. Developer Tools are absent by default and become available only when the owner starts the host with `DOMINION_DESKTOP_DEVTOOLS=1`. The one exposed native command is restricted to the main window and its exact owned loopback origin; it cannot run a shell or read files.

## Optional source workspace

The installed app is fully usable without a source checkout. To edit and rebuild the source, connect a source directory from DOMINION's development studio, or add `sourceWorkspace` to app-data `desktop.json`. This is an explicit development path and does not replace bundled interface resources or private app data. Moving that optional checkout does not prevent the app from starting. Ordinary task files use `workspace`, which can also be set in that file.

```json
{
  "workspace": "/your/writable/tasks",
  "sourceWorkspace": "/your/optional/dominion-rust"
}
```

For isolated smoke testing, launch the app executable with `DOMINION_DESKTOP_STANDALONE=1` and `DOMINION_DESKTOP_DATA` pointing at a fresh temporary directory. This bypasses the existing development gateway and cannot reuse its private tokens, databases, or bot settings. These environment settings are testing overrides, not bundled paths. On macOS use the canonical executable path (for example `/private/tmp/…` rather than the `/tmp` symlink); Tauri intentionally rejects symlinked executable paths.

Official API references: [sidecars](https://v2.tauri.app/develop/sidecar/), [resources](https://v2.tauri.app/develop/resources/), [native menus](https://docs.rs/tauri/latest/tauri/menu/struct.PredefinedMenuItem.html), [capabilities](https://v2.tauri.app/security/capabilities/), [DMG](https://v2.tauri.app/distribute/dmg/), [Windows installers](https://v2.tauri.app/distribute/windows-installer/).
