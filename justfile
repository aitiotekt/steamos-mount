set windows-shell := ['pwsh', '-NoLogo', '-Command']

dev-tauri-app:
    cd apps/tauri-app && pnpm tauri dev

build-tauri-app:
    cd apps/tauri-app && pnpm tauri build

prepare-on-archlinux:
    yay -S --needed --noconfirm dpkg rpm-org patchelf squashfs-tools xorg-server-xvfb
    cargo install tauri-cli --git https://github.com/aitiotekt/tauri --branch feat/truly-portable-appimage --force

build-tauri-app-on-archlinux:
    cd apps/tauri-app && TAURI_BUNDLER_NEW_APPIMAGE_FORMAT=true NO_STRIP=true cargo tauri build