#!/bin/sh
# Builds dist/infinity-mouse-settings_<version>_amd64.deb and a .tar.gz.
# Needs cargo-zigbuild (pip install ziglang cargo-zigbuild) so the binary
# runs on glibc 2.35 and newer (Ubuntu 22.04+, Debian 12+).
set -eu
cd "$(dirname "$0")/.."

NAME=infinity-mouse-settings
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
TARGET=x86_64-unknown-linux-gnu
BIN=target/$TARGET/release/$NAME

cargo zigbuild --release --target "$TARGET.2.35"

rm -rf dist && mkdir -p dist

# ---- .deb ----
PKG=dist/deb
install -Dm755 "$BIN" "$PKG/usr/bin/$NAME"
install -Dm644 packaging/$NAME.desktop "$PKG/usr/share/applications/$NAME.desktop"
install -Dm644 packaging/$NAME.svg "$PKG/usr/share/icons/hicolor/scalable/apps/$NAME.svg"
install -Dm644 packaging/70-am-infinity-mouse.rules "$PKG/usr/lib/udev/rules.d/70-am-infinity-mouse.rules"
install -Dm644 LICENSE "$PKG/usr/share/doc/$NAME/copyright"
install -Dm644 NOTICE.md "$PKG/usr/share/doc/$NAME/NOTICE.md"
install -Dm755 packaging/deb/postinst "$PKG/DEBIAN/postinst"
install -Dm755 packaging/deb/postrm "$PKG/DEBIAN/postrm"
SIZE=$(du -sk --exclude=DEBIAN "$PKG" | cut -f1)
cat > "$PKG/DEBIAN/control" <<CTRL
Package: $NAME
Version: $VERSION
Section: utils
Priority: optional
Architecture: amd64
Installed-Size: $SIZE
Depends: libc6 (>= 2.35), libgcc-s1, libegl1, libxkbcommon0
Recommends: libwayland-client0, libwayland-egl1, libx11-xcb1, libxcursor1, libxi6, libxkbcommon-x11-0
Maintainer: hieuuk <78854+hieuuk@users.noreply.github.com>
Homepage: https://github.com/hieuuk/$NAME
Description: Unofficial settings app for the AngryMiao AM Infinity mouse
 Changes DPI levels, button assignments, polling rate, debounce, lift-off
 distance and sensor options on the AngryMiao AM Infinity mouse. Settings are
 stored on the mouse. Not affiliated with or endorsed by AngryMiao.
CTRL
dpkg-deb --root-owner-group --build "$PKG" "dist/${NAME}_${VERSION}_amd64.deb"
rm -rf "$PKG"

# ---- .tar.gz (any distribution) ----
T=dist/${NAME}-${VERSION}-x86_64-linux
mkdir -p "$T"
cp "$BIN" packaging/$NAME.desktop packaging/$NAME.svg packaging/70-am-infinity-mouse.rules LICENSE NOTICE.md "$T/"
tar -C dist -czf "$T.tar.gz" "$(basename "$T")"
rm -rf "$T"

(cd dist && sha256sum ./*.deb ./*.tar.gz > SHA256SUMS)
ls -la dist
