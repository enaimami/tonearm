# AUR packages

Three PKGBUILDs, four packages for Arch Linux:

| Directory | AUR name | What it does |
|---|---|---|
| `headshell/` | `headshell` + `headshell-cli` | Downloads the release tag, builds from source |
| `headshell-bin/` | `headshell-bin` | Installs the desktop binary from the release's `.deb` |
| `headshell-cli-bin/` | `headshell-cli-bin` | Installs the CLI binary from the release's archive |

**Only the source package is split**, and there is a reason: the two binaries
come out of a single `cargo build`; with separate PKGBUILDs a user installing
both would build ~500 crates twice. On the `-bin` side there is no shared
build, so the two stand apart — that way a user installing only the CLI
doesn't download the 10 MB `.deb`, and namcap's `splitpkgmakedeps` error never
comes up (a split PKGBUILD's global `makedepends` must cover the sub-packages'
`depends`; on the `-bin` side those dependencies belong only to run time).

The `-bin` packages cover their neighbours with `provides`/`conflicts`, so
`headshell` and `headshell-bin` can't be installed at the same time — they
install the same files anyway.

This directory is the packages' **source**; the AUR's own git repositories are
separate (see below). The reason it is kept here is that the package is
versioned together with the repository: when the `.desktop` entry, the icon
names or the dependencies change, the PKGBUILD changes in the same commit.

## Where the shared files come from

The desktop entry **is not written separately**: `headshell` and
`headshell-bin` install the same `packaging/headshell.desktop`. For this the
`-bin` packages download the source archive too (1.2 MB) — the icons and the
license files come from there as well; the CLI archive contains only the
binary. A second hand-written copy would drift apart over time; this project's
log has three records of the same failure (D-062, D-063).

`StartupWMClass=headshell-desktop` isn't a made-up value: the entry Tauri
generates for the `.deb` was opened and read, and the window class is derived
from the binary name. If `headshell` were written, the running window wouldn't
match the launcher icon.

## When publishing a release

The order matters, because every step changes the input of the next.

1. **The tag first, then the PKGBUILD.** `source=` points at the tag's archive,
   and `-bin` at the release assets; neither exists before the tag is pushed.
2. **The release assets must be published.** `release.yml` opens a **draft**
   release, and a draft's assets can't be downloaded anonymously — only those
   with access to the repository see them. That's why the `-bin` package
   doesn't work until the release leaves draft. The source package has no such
   debt: the tag archive is independent of the draft and public as long as the
   repository is.
3. **Update `pkgver`.** One line: `pkgver=X.Y.Z`. Since `-` is the pkgrel
   separator in an Arch version, pre-releases are written with `_`
   (`0.0.1_beta`); the upstream spelling is derived with `_pkgver`. Reset
   `pkgrel=1`.
4. **Refresh the checksums:**
   ```bash
   updpkgsums          # pacman-contrib
   ```
5. **Generate `.SRCINFO`** (the AUR requires it, and it isn't written by
   hand):
   ```bash
   makepkg --printsrcinfo > .SRCINFO
   ```
6. **Test:**
   ```bash
   makepkg -si --clean
   namcap PKGBUILD *.pkg.tar.zst
   ```
7. **Push to the AUR:**
   ```bash
   git clone ssh://aur@aur.archlinux.org/headshell.git aur-headshell
   cp PKGBUILD aur-headshell/
   cd aur-headshell && makepkg --printsrcinfo > .SRCINFO
   git add PKGBUILD .SRCINFO && git commit && git push
   ```
   There are three AUR repositories — `headshell` (split, two packages),
   `headshell-bin`, `headshell-cli-bin`. The version goes up in all three.

`.SRCINFO` **is not kept in this repository**: it lives in the AUR repository
and is generated there from `makepkg`. A copy here would silently go stale when
the PKGBUILD changed.

## Testing in a container

The PKGBUILDs are **not at the repository root** but under the directories
above — if you run `makepkg` at the root, you'll be told "PKGBUILD does not
exist". The `Makefile` at the repository root goes into the right directory
itself:

```bash
make aur-test PKG=headshell
make aur-test PKG=headshell-bin
make aur-test PKG=headshell-cli-bin
make aur-clean                     # delete the leftovers
```

On an Arch machine `makepkg` runs **directly**; on a machine without `makepkg`
the same job is done in an Arch container (this Makefile was written on
Debian). The choice is automatic; to force it, `ENGINE=native` or
`ENGINE=container`. A native run needs `pacman-contrib` (`updpkgsums`) and
`namcap`; if they're missing, the target stops, saying what is missing and what
to type.

The target does the following in order: it produces a **tag-equivalent** source
archive from the working tree, fetches the release assets for the `-bin`
packages with `gh`, copies all of them under `target/aur/<package>/`, refreshes
the checksums against those local files with `updpkgsums` in the container,
runs `makepkg` and `namcap`s the result. **It doesn't touch the PKGBUILD in the
repository** — that keeps carrying the real tag's checksums.

The tag-equivalent archive is needed because `source=` points at the tag,
while what you want to test is your working tree, not yet pushed. When the
archive sits next to the PKGBUILD, `makepkg` skips the download.

If you run it by hand, there are two traps: `makepkg` doesn't run as root, and
`-s` wants `sudo` — in the container, install the makedepends as root first.
And in the container, files left behind by `fakeroot` end up owned by root on
the host; clean up with `podman unshare rm -rf` (`make aur-clean` tries both).

## Why not `cargo tauri build`

`cargo tauri build` is a bundler: it produces `.deb`, `.rpm`, `.AppImage`. The
Arch package is already installed by `makepkg`, so the bundler's job would be
repeated — and `tauri-cli` would come along as a build dependency. A plain
`cargo build` is enough; in return, the PKGBUILD installs the `.desktop` entry
and the icons by hand.

## The plugins have no run-time dependency

`python` and `yt-dlp` left `optdepends` (D-069): the plugin engine (QuickJS) is
inside the binary, and the yt-dlp the YouTube Music plugin wants is downloaded
by the engine, as the platform's self-contained binary, into the user's data
directory. The system's `yt-dlp` package isn't used — the manifest pins the
version and its hash is verified.

The torrent plugin was parked (`parked/`, D-069) and goes into no package.
