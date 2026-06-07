# Packaging

Arch Linux packaging for rift. These files build the workspace, bundle the KWin
script, and install everything system-wide with a `systemd --user` unit.

| File           | Purpose                                                        |
| -------------- | -------------------------------------------------------------- |
| `PKGBUILD`     | The build recipe (`prepare`/`build`/`check`/`package`).        |
| `.SRCINFO`     | Generated metadata (`makepkg --printsrcinfo`). Keep in sync.   |
| `rift.install` | Post-install / post-upgrade hints (points at `riftctl setup`). |
| `riftd.service`| The `systemd --user` unit installed to `/usr/lib/systemd/user`.|

## Source is pinned to a commit, not a tag (on purpose)

rift is still pre-release and changing often, so **`source` points at a specific
commit archive rather than a `v$pkgver` tag**:

```sh
_commit=<full-40-char-sha>
source=("$pkgname-$pkgver.tar.gz::$url/archive/$_commit.tar.gz")
sha256sums=('<sha256 of that tarball>')
```

Why this, rather than `archive/refs/tags/v0.1.0.tar.gz` with `SKIP`:

- **No premature tag.** We want to be more feature-complete and better tested
  before cutting a real `v0.1.0`. A commit pin lets the package track a known-good
  revision without claiming a release.
- **A real checksum, not `SKIP`.** A commit archive is downloadable today, so its
  `sha256` can be pinned and verified. `SKIP` disables integrity checking and is
  discouraged for anything but a moving VCS source.
- **Reproducible.** A commit archive is immutable; a branch archive
  (`archive/refs/heads/main.tar.gz`) would change underfoot.

The commit archive extracts to `rift-<commit>/`, which is why the build functions
`cd "$pkgname-$_commit"` (not `"$pkgname-$pkgver"`).

## Updating the pinned commit

After pushing new commits you want the package to track:

```sh
cd packaging
_commit=$(git rev-parse HEAD)
# Point the recipe at the new commit, then refresh the checksum + metadata:
sed -i "s/^_commit=.*/_commit=$_commit/" PKGBUILD
updpkgsums                       # downloads the tarball, rewrites sha256sums
makepkg --printsrcinfo > .SRCINFO
```

`Cargo.lock` is committed to the repo, so the GitHub archive includes it and the
`--locked` / `--frozen` cargo steps work offline after `prepare()`.

## Cutting a real release (when ready)

Once rift is feature-complete and tested enough for a tagged release:

1. Tag and push: `git tag -a v0.1.0 -m "rift 0.1.0" && git push origin v0.1.0`.
2. Switch the recipe back to the tag tarball and drop the `_commit` indirection:
   ```sh
   source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/v$pkgver.tar.gz")
   ```
   and change the `cd "$pkgname-$_commit"` lines back to `cd "$pkgname-$pkgver"`
   (the tag archive extracts to `rift-$pkgver/`).
3. `updpkgsums && makepkg --printsrcinfo > .SRCINFO`.

## Build / validate locally

```sh
# In the packaging dir (downloads the pinned tarball and verifies the sha):
makepkg -f

# Clean-room check in a container — build from a real `git archive`, never a
# `tar` of the working tree (a working-tree tar can hide gitignored files such
# as a missing Cargo.lock that the real forge tarball would not contain):
git archive --format=tar.gz --prefix="rift-$(git rev-parse HEAD)/" HEAD \
  -o rift-0.1.0.tar.gz
# ...then makepkg against that tarball inside archlinux:latest.
```

## Post-install

The package can't touch per-user KDE state at install time, so after installing
run the per-user setup once in a Plasma session:

```sh
riftctl setup
```

See [`../docs/install.md`](../docs/install.md) for the full install flow.
