#!/bin/bash
# Installs upstream Git under /usr/local in a workbench, as root. lfs-vm.sh runs it at each
# create; it is safe to run again.
#   git-install.sh
# Bookworm's Git is 2.39.5, and memoryd needs 2.42 or later (`git var GIT_CONFIG_SYSTEM`); no
# Debian package for bookworm is newer. The build happens once: its staged /usr/local tree is
# kept on the data volume, so a recreate only unpacks it. Debian's Git stays in /usr/bin;
# /usr/local/bin comes first on PATH. Undo: rm /usr/local/bin/git.
set -euo pipefail
umask 022 # OrbStack's docker exec starts with umask 0000.

version=2.56.1
sha256=f0b0bf89264b9bd2e1c640363e7529643989da76281600473c50745cd34a607f
build_options=(prefix=/usr/local sysconfdir=/etc NO_GETTEXT=1 NO_TCLTK=1 USE_LIBPCRE2=1 INSTALL_SYMLINKS=1 NO_RUST=1)
cache=/var/lib/layerfs-data/cache/git-$version-$(uname -m).tar

if [ "$(/usr/local/bin/git --version 2>/dev/null || true)" = "git version $version" ]; then
  echo "git $version: present"
  exit 0
fi

work=$(mktemp -d)
trap 'rm -rf "${work:?}"' EXIT
if [ ! -f "$cache" ]; then
  curl -fsSL -o "$work/git.tar.xz" "https://mirrors.edge.kernel.org/pub/software/scm/git/git-$version.tar.xz"
  echo "$sha256  $work/git.tar.xz" | sha256sum -c - >/dev/null
  mkdir "$work/src"
  tar -xJf "$work/git.tar.xz" -C "$work/src" --strip-components=1 --no-same-owner
  nice -n 19 make -s -C "$work/src" -j2 "${build_options[@]}" all >/dev/null
  make -s -C "$work/src" "${build_options[@]}" INSTALL_STRIP=-s DESTDIR="$work/stage" install >/dev/null
  mkdir -p "$(dirname "$cache")"
  tar -C "$work/stage/usr/local" -cf "$cache.part" .
  mv "$cache.part" "$cache"
fi

# Agents may run Git during this step: `git` goes last, so it never appears without its helpers.
mkdir "$work/tree"
tar -xf "$cache" -C "$work/tree"
for entry in "$work/tree"/*; do
  [ "$(basename "$entry")" = bin ] || cp -a "$entry" /usr/local/
done
find "$work/tree/bin" -mindepth 1 -maxdepth 1 ! -name git -exec cp -a {} /usr/local/bin/ \;
cp -aL "$work/tree/bin/git" /usr/local/bin/.git.new
mv -f /usr/local/bin/.git.new /usr/local/bin/git
/usr/local/bin/git var GIT_CONFIG_SYSTEM >/dev/null
echo "git $version: installed"
