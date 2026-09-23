#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
test "$(rustc --version | cut -d' ' -f2)" = 1.92.0
dx --version | grep -q '0.7.9'
cargo fmt --check
cargo check --locked
cargo check --locked --features server --no-default-features
cargo test --locked --features server --no-default-features
cargo check --locked --target wasm32-unknown-unknown
rm -rf target/dx/vmq_mvp/release/web
dx build --release --web --cargo-args=--locked
version=$(python3 -c 'import re; print(re.search(r"^version = \"(.*?)\"", open("Cargo.toml").read(), re.M)[1])')
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
app="$stage/ListeningExamGenerator.AppDir"
mkdir -p "$app/usr/bin" "$app/usr/lib" "$app/usr/share/licenses" dist
cp target/dx/vmq_mvp/release/web/server "$app/usr/bin/"
cp -r target/dx/vmq_mvp/release/web/public "$app/usr/bin/"
cp packaging/linux/AppRun packaging/linux/*.desktop packaging/linux/*.svg "$app/"
chmod +x "$app/AppRun" "$app/usr/bin/server"
ldd "$app/usr/bin/server" | tee dist/linux-dependencies.txt
if grep -q 'not found' dist/linux-dependencies.txt; then
    echo 'Unresolved server shared-library dependency' >&2
    exit 1
fi
while read -r lib; do
    case "$(basename "$lib")" in
        libc.so.*|libm.so.*|libpthread.so.*|libdl.so.*|librt.so.*|ld-linux*) continue ;;
    esac
    cp -L "$lib" "$app/usr/lib/"
    package=$(dpkg-query -S "$lib" "$(readlink -f "$lib")" 2>/dev/null | head -n1 | cut -d: -f1 || true)
    if [ -n "$package" ] && [ -f "/usr/share/doc/$package/copyright" ]; then
        cp "/usr/share/doc/$package/copyright" "$app/usr/share/licenses/$package.txt"
    else
        echo "Missing dependency copyright notice for $lib" >&2
        exit 1
    fi
done < <(ldd "$app/usr/bin/server" | awk '/=> \// {print $3}')
cp /etc/ssl/certs/ca-certificates.crt "$app/usr/share/"
cp /usr/share/doc/ca-certificates/copyright "$app/usr/share/licenses/ca-certificates.txt"
cp -r packaging/linux/licenses "$app/usr/share/licenses/AppImage-runtime"
python3 packaging/notices.py "$app/usr/share/licenses/RUST-DEPENDENCIES.txt"
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 /opt/appimagetool --runtime-file /opt/runtime-x86_64 "$app" "dist/listening-exam-generator-$version-linux-x86_64.AppImage"
cp packaging/PORTABLE-README.txt dist/README.txt
cd dist
sha256sum *.AppImage > SHA256SUMS-linux.txt
