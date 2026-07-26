# Pake online bootstrap

Experimental online mode reuses the Pake Rust binary and the normal Tauri
bundlers. It does not contain a separate Tauri, Qt, or webview installer
project.

The workflow builds two variants from the same application configuration:

1. The real application payload: a ZIP-compressed executable on Windows, a
   zipped application bundle on macOS, or an AppImage on Linux.
2. A windowless `online-bootstrap` build, packaged as a normal MSI, DMG, or
   AppImage with the same application name, icon, and native installer layout.

On first launch, the bootstrap downloads and verifies the newest payload from
its rolling GitHub Release channel, stores it in the current user's data
directory, and launches it. On later launches, it starts the cached application
immediately and checks for an update in the background. A downloaded update
becomes active on the next launch.

The bootstrap accepts only HTTPS assets from its embedded public GitHub
repository and release tag. It verifies the manifest, byte size, and SHA-256
before activating a payload. GitHub downloads use the configured proxy only
when Cloudflare reports that the client is in mainland China, with a direct
GitHub fallback.
