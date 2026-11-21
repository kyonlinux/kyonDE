pkgname=kyonde
pkgver=0.1.0
pkgrel=3
pkgdesc="KyonDE Wayland desktop environment with a Smithay compositor and GTK4 shell"
arch=('x86_64' 'aarch64')
url="https://github.com/kyonde/kyonde"
license=('MIT')
depends=(
  'gtk4'
  'gtk4-layer-shell'
  'libinput'
  'seatd'
  'systemd-libs'
  'libdrm'
  'mesa'
  'wayland'
  'libxkbcommon'
  'fontconfig'
  'freetype2'
  'libxcb'
  'libglvnd'
  'hicolor-icon-theme'
)
makedepends=('cargo' 'rust' 'pkgconf' 'clang' 'git')
optdepends=(
  'vulkan-icd-loader: Vulkan device probing'
  'nvidia-utils: NVIDIA GPU rendering'
  'noto-fonts: UI font fallback'
  'libx11: nested backend inside an X11 session'
)
source=("kyonde-$pkgver.tar.gz")
sha256sums=('SKIP')
options=(!lto)

build() {
  cd "$srcdir/kyonde-$pkgver"
  export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
  cargo build --release -p kyonde -p kyonshell -p kyonctl
}

package() {
  cd "$srcdir/kyonde-$pkgver"
  install -d "$pkgdir/usr/bin" "$pkgdir/usr/share/kyonde" \
    "$pkgdir/usr/share/applications" "$pkgdir/usr/share/wayland-sessions" \
    "$pkgdir/usr/share/doc/kyonde"
  install -m 0755 target/release/kyonde target/release/kyonshell target/release/kyonctl "$pkgdir/usr/bin/"
  install -m 0755 scripts/kyon-session "$pkgdir/usr/bin/kyon-session"
  install -m 0644 examples/config.lua "$pkgdir/usr/share/kyonde/config.lua"
  install -m 0644 LICENSE README.md "$pkgdir/usr/share/doc/kyonde/"
  sed "s|@BIN_DIR@|/usr/bin|g" data/kyonshell-settings.desktop.in > "$pkgdir/usr/share/applications/kyonshell-settings.desktop"
  sed "s|@BIN_DIR@|/usr/bin|g" data/kyonshell-launcher.desktop.in > "$pkgdir/usr/share/applications/kyonshell-launcher.desktop"
  sed "s|@BIN_DIR@|/usr/bin|g" data/kyonde-nested.desktop.in > "$pkgdir/usr/share/applications/kyonde-nested.desktop"
  install -m 0644 data/kyonde-session.desktop "$pkgdir/usr/share/wayland-sessions/kyonde.desktop"
}
