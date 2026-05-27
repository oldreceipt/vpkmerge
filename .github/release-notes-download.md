## Download

Two flavors below: most people want the **desktop app**. Power users / scripters want the **CLI**.

### Desktop app (recommended)

| Your computer | File |
|---|---|
| **Windows** | [`vpkmerge_@VER@_x64-setup.exe`](@BASE@/vpkmerge_@VER@_x64-setup.exe) (installer), [`vpkmerge_@VER@_x64_en-US.msi`](@BASE@/vpkmerge_@VER@_x64_en-US.msi), or [`vpkmerge_@VER@_x64_portable.exe`](@BASE@/vpkmerge_@VER@_x64_portable.exe) (portable, no install) |
| **macOS** (Apple Silicon, M1+) | [`vpkmerge_@VER@_aarch64.dmg`](@BASE@/vpkmerge_@VER@_aarch64.dmg) |
| **macOS** (Intel) | Run the Apple Silicon `.dmg` under Rosetta 2, or build from source. |
| **Linux** (Debian / Ubuntu) | [`vpkmerge_@VER@_amd64.deb`](@BASE@/vpkmerge_@VER@_amd64.deb) |
| **Linux** (Fedora / RHEL)   | [`vpkmerge-@VER@-1.x86_64.rpm`](@BASE@/vpkmerge-@VER@-1.x86_64.rpm) |
| **Linux** (anything else)   | [`vpkmerge_@VER@_amd64.AppImage`](@BASE@/vpkmerge_@VER@_amd64.AppImage) (no install, `chmod +x` and run) |

Install steps:
- **Windows**: double-click the `.exe` installer, or just run `vpkmerge_@VER@_x64_portable.exe` directly (no install; needs WebView2, preinstalled on Windows 10 1903+ and all Windows 11).
- **macOS**: open the `.dmg`, drag the app into Applications. First launch: right-click then Open (app is not notarized).
- **Linux .deb**: `sudo dpkg -i vpkmerge_@VER@_amd64.deb`
- **Linux .rpm**: `sudo dnf install ./vpkmerge-@VER@-1.x86_64.rpm`
- **Linux AppImage**: `chmod +x vpkmerge_@VER@_amd64.AppImage && ./vpkmerge_@VER@_amd64.AppImage`

### Command-line tool

| Your computer | File |
|---|---|
| **Linux** (x86_64) | [`vpkmerge-linux-x86_64`](@BASE@/vpkmerge-linux-x86_64) |
| **macOS** (Apple Silicon) | [`vpkmerge-macos-aarch64`](@BASE@/vpkmerge-macos-aarch64) |
| **Windows** (x86_64) | [`vpkmerge-windows-x86_64.exe`](@BASE@/vpkmerge-windows-x86_64.exe) |

On Linux/macOS, `chmod +x vpkmerge-*` once and call it from a terminal. On Windows it's already executable.

---
