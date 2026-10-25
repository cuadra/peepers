# Maintainer: Jose Cuadra <jose.cuadra@live.com>
pkgname=peepers
pkgver=0.1.0
pkgrel=1
pkgdesc="A hyper-minimal, vim-motion image viewer for Wayland"
arch=('x86_64' 'aarch64')
url="https://github.com/cuadra/peepers"
license=('MIT' 'OFL-1.1')
depends=('gcc-libs' 'glibc' 'libxkbcommon' 'wayland' 'vulkan-icd-loader')
makedepends=('cargo')
optdepends=(
  'vulkan-driver: GPU rendering (falls back to CPU rendering without it)'
  'libx11: run under X11 / XWayland'
  'libxcursor: X11 cursor support'
  'libxrandr: X11 multi-monitor support'
  'libxi: X11 input support'
)
options=('!lto')
source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('SKIP')

prepare() {
  cd "$pkgname-$pkgver"
  export RUSTUP_TOOLCHAIN=stable
  cargo fetch --locked --target "$(rustc -vV | sed -n 's/host: //p')"
}

build() {
  cd "$pkgname-$pkgver"
  export RUSTUP_TOOLCHAIN=stable
  export CARGO_TARGET_DIR=target
  cargo build --frozen --release
}

check() {
  cd "$pkgname-$pkgver"
  export RUSTUP_TOOLCHAIN=stable
  cargo test --frozen --release
}

package() {
  cd "$pkgname-$pkgver"
  install -Dm755 "target/release/$pkgname" "$pkgdir/usr/bin/$pkgname"
  install -Dm644 assets/peepers.desktop "$pkgdir/usr/share/applications/peepers.desktop"
  install -Dm644 README.md "$pkgdir/usr/share/doc/$pkgname/README.md"
  install -Dm644 extras/config.toml "$pkgdir/usr/share/doc/$pkgname/config.toml"
  install -Dm644 extras/hyprland.conf "$pkgdir/usr/share/doc/$pkgname/hyprland.conf"
  install -Dm644 extras/yazi.toml "$pkgdir/usr/share/doc/$pkgname/yazi.toml"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
  install -Dm644 assets/fonts/OFL-Instrument.txt "$pkgdir/usr/share/licenses/$pkgname/OFL-Instrument.txt"
  install -Dm644 assets/fonts/OFL-JetBrainsMono.txt "$pkgdir/usr/share/licenses/$pkgname/OFL-JetBrainsMono.txt"
}
