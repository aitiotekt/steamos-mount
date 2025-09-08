set windows-shell := ['pwsh', '-NoLogo', '-Command']

dev-tauri-app:
    cd apps/tauri-app && pnpm tauri dev

build-tauri-app:
    cd apps/tauri-app && pnpm tauri build

prepare-on-archlinux:
    yay -S --needed --noconfirm dpkg rpm-org patchelf squashfs-tools
    cargo install tauri-cli --git https://github.com/tauri-apps/tauri --branch feat/truly-portable-appimage

build-tauri-app-on-archlinux:
    cd apps/tauri-app && TAURI_BUNDLER_NEW_APPIMAGE_FORMAT=true NO_STRIP=true pnpm tauri build