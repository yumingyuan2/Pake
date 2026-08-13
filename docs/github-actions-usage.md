# GitHub Actions Usage Guide

<h4 align="right"><strong>English</strong> | <a href="github-actions-usage_CN.md">简体中文</a></h4>

Build Pake apps online without installing development tools locally.

## Quick Steps

### 1. Fork Repository

[Fork this project](https://github.com/tw93/Pake/fork)

### 2. Run Workflow

1. Go to Actions tab in your forked repository
2. Select `Build App With Pake CLI`
3. Fill in the form (same parameters as [CLI options](cli-usage.md))
4. Click `Run Workflow`

   ![Actions Interface](https://raw.githubusercontent.com/tw93/static/main/pake/action.png)

### 3. Download App

- Green checkmark = build success
- Click the workflow name to view details
- Find `Artifacts` section and download your app

  ![Build Success](https://raw.githubusercontent.com/tw93/static/main/pake/action2.png)

### 4. Build Times

- **First run**: ~10-15 minutes (sets up cache)
- **Subsequent runs**: ~5 minutes (uses cache)
- Cache size: 400-600MB when complete

### Optional Windows Offline EXE

Select `offline_exe` to publish an additional `.exe` that embeds the generated
MSI and launches Windows Installer with its native UI. The MSI remains
available as the regular offline package.

`offline_exe_icon` and `online_exe_icon` are independent icon URLs for the
offline wrapper and experimental Windows online installer. ICO files are used
directly; SVG, PNG, JPEG, and other Sharp-supported images are converted to
ICO. Icon URLs must use HTTP(S), cannot contain credentials, and are limited to
10 MiB.

## Experimental Online Mode

Select `online_mode` when running `Build App With Pake CLI` to register the
current form values for the selected branch. The first run builds immediately;
each later push to that same branch rebuilds every registered configuration and
updates its rolling prerelease.

For every online-mode build, the application version is automatically set to
the latest stable Pake Release version. In a fork, the workflow reads the
latest Release from its upstream parent repository. The manual `app_version`
value continues to apply to non-online builds.

The prerelease contains a compressed real application payload plus a lightweight
online bootstrap. The bootstrap is a windowless Pake build, not a second Tauri
project or an external installer framework. It is packaged by the same native
Tauri bundler as an offline build, so its application name, icon, shortcuts, and
installer layout remain consistent with the offline package.

- Windows: `online_windows_format` selects an app-specific `.msi` or `.exe`.
  The MSI is the normal Pake WiX package. The EXE is a completely windowless
  wrapper around that same MSI, and `online_exe_icon` controls the wrapper icon.
  The bootstrap uses version `255.0.0`; the real application payload uses the
  latest stable Pake Release version.
- macOS: the `.dmg` is produced by the normal Pake DMG bundler and contains an
  app with the configured name and icon.
- Linux: the `.AppImage` is produced by the normal Pake AppImage bundler and
  uses the configured application name and icon.

On the first application launch, the bootstrap silently downloads, verifies,
and starts the newest real application. On later launches it starts the cached
application immediately, checks for updates in the background, and activates a
downloaded update on the next launch. There is no console window, progress
window, or second installer UI.

For online-mode runs, the Actions **Artifacts** section contains only the
online installer. Open the rolling prerelease when you also need the real
payload or native package. Non-online runs continue to upload only their
regular offline packages as three-day Actions artifacts.

Windows payloads are ZIP-compressed executables, macOS payloads are zipped app
bundles, and Linux payloads remain compressed AppImages. The bootstrap accepts
only HTTPS assets from its embedded repository and rolling Release tag, then
verifies the manifest, byte size, and SHA-256 before activation. The rolling
Release retains the current and previous successful payloads and manifests.

Before a GitHub asset download, the bootstrap queries Cloudflare's country
trace. In mainland China it tries
`https://v4.gh-proxy.org/https://github.com/owner/repo/releases/download/...`
and falls back to GitHub directly; elsewhere it uses GitHub directly.

### Requirements and Limits

- Online mode is experimental and supports public forks only. No GitHub token
  is stored in the configuration or installer.
- In **Settings → Actions → General → Workflow permissions**, allow read and
  write access so the workflow can maintain its configuration branch and
  prereleases.
- Configurations are keyed by app name, platform, and source branch. Running
  the same combination with `enable-or-update` replaces its saved values.
- Select `pause` with the same app, platform, and branch to stop future push
  builds. The last prerelease remains available.
- Saved configurations live on `pake-online-config`. Each configuration
  consumes a runner on every matching push.
- Native packages install the bootstrap normally. Downloaded real applications
  are kept in the current user's Pake data directory on every platform.

## Tips

- Be patient on first run - let cache build completely
- Stable network connection recommended
- Enable `Allow sites to open new windows` when the site launches sign-in,
  exam, or other flows in a separate window
- If build fails, delete cache and retry

## Links

- [CLI Documentation](cli-usage.md)
- [Advanced Usage](advanced-usage.md)
