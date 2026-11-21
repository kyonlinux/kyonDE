#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
VERSION="0.1.0"
rm -rf dist
mkdir -p dist/arch
tar --exclude=target --exclude=.git --exclude=dist --exclude=smithay/.git --exclude=smithay/target \
    -czf "dist/kyonde-$VERSION.tar.gz" --transform "s,^,kyonde-$VERSION/," .
cp PKGBUILD dist/arch/PKGBUILD
cp "dist/kyonde-$VERSION.tar.gz" dist/arch/
cd dist/arch
makepkg --force --noconfirm --nodeps
