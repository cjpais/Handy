pkgname=handy
pkgver=0.9.3
pkgrel=1
pkgdesc="A free, open source, and extensible speech-to-text application that works offline"
arch=('x86_64')
url="https://github.com/cjpais/Handy"
license=('MIT')
depends=(
  'gtk3'
  'webkit2gtk-4.1'
  'gtk-layer-shell'
  'libayatana-appindicator'
  'openblas'
)
makedepends=(
  'binutils'
  'coreutils'
)
source=("handy-${pkgver}.deb::https://github.com/cjpais/Handy/releases/download/v${pkgver}/Handy_${pkgver}_amd64.deb")
sha256sums=('SKIP')
noextract=("handy-${pkgver}.deb")

build() {
  return 0
}

prepare() {
  cd "$srcdir"
  ar x "handy-${pkgver}.deb" data.tar.gz
  tar -xzf data.tar.gz
}

package() {
  cd "$srcdir"
  install -Dm755 usr/bin/handy "${pkgdir}/usr/bin/handy"
  cp -a usr/lib/Handy "${pkgdir}/usr/lib/"
  cp -a usr/lib/*.so* "${pkgdir}/usr/lib/"
  install -Dm644 usr/share/applications/Handy.desktop "${pkgdir}/usr/share/applications/Handy.desktop"
  cp -a usr/share/icons/hicolor "${pkgdir}/usr/share/icons/"
}

