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

## Release workflow (automated)

`vpkmerge-bin` and `vpkmerge-cli-bin` are bumped and pushed to the AUR
automatically. When a tag `vX.Y.Z` triggers `.github/workflows/release.yml`, its
final `aur` job calls the reusable `.github/workflows/aur.yml` **after** the
GitHub release is published (so the .deb and CLI binary URLs resolve). That
workflow:

1. **`sync`** (Arch container): for each package it runs the exact manual recipe
   below (`sed` the `pkgver`, `updpkgsums`, `makepkg --printsrcinfo`) and commits
   the refreshed `PKGBUILD` + `.SRCINFO` back to `main` (`[skip ci]`), keeping
   this in-repo mirror authoritative.
2. **`publish`**: pushes each package to its AUR repo
   (via `KSXGitHub/github-actions-deploy-aur`).

### One-time CI setup

Add one repo secret so the `publish` job can push over SSH:

- `AUR_SSH_PRIVATE_KEY`: a private key whose public half is registered on the AUR
  account (same key as the local `~/.ssh/aur` below). Settings -> Secrets and
  variables -> Actions -> New repository secret.

Notes:
- The AUR repos must already exist (first import is still manual, see above). The
  action pushes to `ssh://aur@aur.archlinux.org/<pkgname>.git`.
- The `sync` job pushes a commit to `main`. If `main` ever gets branch
  protection that blocks the Actions bot, either allow it or drop the commit-back
  step (the AUR push does not depend on it).
- If the secret is missing, `publish` warns and skips (the release itself still
  succeeds); set the secret and re-run the job.

### Running it by hand

To (re)publish a specific tag (e.g. the first run after adding the secret, or to
backfill v0.4.0): Actions -> **AUR** -> Run workflow -> enter the tag (`v0.4.0`).
This works for any already-published release.

### The manual recipe (fallback)

Still valid if you need to push outside CI, or after a structural PKGBUILD change
(new deps, build steps, .desktop content) that the version-bump automation does
not cover. For each of `vpkmerge-bin/` and `vpkmerge-cli-bin/`:

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

`vpkmerge-git` doesn't need version bumps; AUR helpers regenerate `pkgver` via the `pkgver()` function on each user build. Only push when the PKGBUILD itself changes (deps, build steps, .desktop content). The automation deliberately leaves it alone.

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
