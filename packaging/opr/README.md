# Omarchy Package Repository recipe

`napkin/` is the recipe for napkin in the
[Omarchy Package Repository](https://github.com/omacom/omarchy-pkgs). It is
kept here so it versions with the app; it is meant to be copied, as is, to
`pkgbuilds/napkin/` in a fork of `omacom/omarchy-pkgs` and sent as a PR.

- `PKGBUILD` builds the tagged release tarball
  (`https://github.com/nerkyator/napkin/archive/refs/tags/v<pkgver>.tar.gz`).
- `napkin.install` refreshes the desktop database and icon cache.
- `.omarchy/package.json` watches GitHub Releases for `v<version>` tags, so
  OPR's bot proposes each later release by itself, checksum included.

`packaging/PKGBUILD` one level up is a different thing: it builds the local
checkout with `makepkg -si`.

## Fill in the checksum after tagging

The archive GitHub serves for a tag only exists once the tag is pushed, and
its bytes depend on the tagged commit, so the checksum can only be written
after the tag. After `git tag v<version> && git push --tags` and the GitHub
release (a full release: the watch skips drafts and prereleases):

```sh
cd packaging/opr/napkin
updpkgsums          # from pacman-contrib; downloads the tarball, writes sha256sums
rm -f napkin-*.tar.gz
makepkg -f          # optional: build it once more against the real tarball
```

Then copy `napkin/` (including the hidden `.omarchy/`) to
`pkgbuilds/napkin/` in the omarchy-pkgs fork and check the watch with
`python helpers/upstream-watch.py check pkgbuilds/napkin`, which should
report `already current`.

For a later release in the same OPR recipe nothing needs doing here: the
watch finds the new tag, sets `pkgver`, resets `pkgrel` and refreshes the
checksum. Only a packaging change at the same version needs a hand edit,
with a `pkgrel` bump.
