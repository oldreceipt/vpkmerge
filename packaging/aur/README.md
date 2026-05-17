# AUR packaging

Three PKGBUILDs live here, one per AUR package:

| Dir | Package | Source | Installs |
|---|---|---|---|
| `vpkmerge-bin/` | `vpkmerge-bin` | Extracts the GH-Release `.deb` | `/usr/bin/vpkmerge` (GUI) + .desktop + icons |
| `vpkmerge-cli-bin/` | `vpkmerge-cli-bin` | Raw `vpkmerge-linux-x86_64` from GH Release | `/usr/bin/vpkmerge-cli` |
| `vpkmerge-git/` | `vpkmerge-git` | `git+https://github.com/Slush97/vpkmerge.git` | Both binaries from HEAD |

`vpkmerge-bin` and `vpkmerge-git` conflict (both `provides=vpkmerge`); `vpkmerge-cli-bin` and `vpkmerge-git` conflict (both `provides=vpkmerge-cli`). `vpkmerge-bin` + `vpkmerge-cli-bin` can coexist (different binary paths).

## One-time AUR setup

1. SSH key. The AUR uses SSH-only git. Upload your public key at <https://aur.archlinux.org/account/> (the "SSH Public Key" field).
2. `~/.ssh/config` block:
    ```
    Host aur.archlinux.org
      User aur
      IdentityFile ~/.ssh/aur
    ```
3. First push of each package creates the AUR repo:
    ```
    git clone ssh://aur@aur.archlinux.org/vpkmerge-bin.git /tmp/aur-vpkmerge-bin
    cp PKGBUILD .SRCINFO /tmp/aur-vpkmerge-bin/
    cd /tmp/aur-vpkmerge-bin && git add -A && git commit -m "Initial import" && git push
    ```

## Release workflow

When you cut a new vpkmerge release (tag `vX.Y.Z` triggers `.github/workflows/release.yml`), the AUR packages need to be updated **after** the GitHub release is published (so the .deb and CLI binary URLs resolve).

For each of `vpkmerge-bin/` and `vpkmerge-cli-bin/`:

```bash
cd packaging/aur/vpkmerge-bin   # or vpkmerge-cli-bin
sed -i "s/^pkgver=.*/pkgver=X.Y.Z/" PKGBUILD
sed -i "s/^pkgrel=.*/pkgrel=1/" PKGBUILD
updpkgsums                                       # fills sha256sums from the live URLs
makepkg --printsrcinfo > .SRCINFO                # MUST be in the commit
makepkg -f                                       # local smoke test
namcap PKGBUILD *.pkg.tar.zst                    # optional lint
```

Then push to the AUR remote for that package (see "One-time AUR setup" above).

`vpkmerge-git` doesn't need version bumps; AUR helpers regenerate `pkgver` via the `pkgver()` function on each user build. Only push when the PKGBUILD itself changes (deps, build steps, .desktop content).

## Why we ship a separate `-cli-bin` package

Headless / scripting users don't want webkit2gtk-4.1, gtk3, and libayatana-appindicator pulled in. Splitting the CLI lets servers and CI runners install just the merge tool.

## Why GUI `.deb` extraction, not AppImage

The GH Release `.deb` already contains a properly-laid-out filesystem (`/usr/bin`, `/usr/share/applications`, `/usr/share/icons/hicolor/...`) produced by tauri-bundler. Extracting it with `bsdtar` and linking against Arch's system `webkit2gtk-4.1` is the cleanest integration: no bundled-AppImage runtime weirdness, no double-installed libs, real `update-desktop-database` / `gtk-update-icon-cache` hooks fire correctly.

## Things to verify before each push

- `makepkg -si` actually installs and `vpkmerge` launches.
- `pacman -Qlp <built-pkg.tar.zst>` lists no unexpected files (especially nothing under `/usr/share/doc/` that should be in `/usr/share/licenses/`).
- `.SRCINFO` matches the PKGBUILD (regenerate; the AUR web UI reads only `.SRCINFO`, not PKGBUILD).
- `namcap` warnings reviewed.
- No em-dashes anywhere (workspace rule).
