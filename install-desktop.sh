#!/usr/bin/env bash
# Install Lagado as a clickable app icon in the Linux application launcher / dock.
# Run once:  ./install-desktop.sh   (then find "Lagado" in your apps, and pin it to the dock/favorites)
# Uninstall: ./install-desktop.sh --uninstall
#
# This is the Linux dev-box integration. Production installs (the Tauri bundle: .deb / AppImage /
# .dmg / .msi) generate the per-OS launcher + icon automatically from tauri.conf.json's bundle.icon.
set -e

REPO="$(cd "$(dirname "$0")" && pwd)"
APPS_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor/512x512/apps"
DESKTOP="$APPS_DIR/lagado.desktop"
ICON="$ICON_DIR/lagado.png"

if [ "$1" = "--uninstall" ]; then
  rm -f "$DESKTOP" "$ICON"
  command -v update-desktop-database >/dev/null && update-desktop-database "$APPS_DIR" 2>/dev/null || true
  echo "Lagado app icon removed."
  exit 0
fi

mkdir -p "$APPS_DIR" "$ICON_DIR"

# Install the logo as the app icon (the 512×512 brand mark).
cp "$REPO/lagado-ui/public/lagado-mark.png" "$ICON"

# Write the desktop entry. Terminal=false → clicks open the app window like any other app.
cat > "$DESKTOP" <<EOF
[Desktop Entry]
Type=Application
Name=Lagado
Comment=Sovereign local AI agent — runs entirely on your machine
Exec=$REPO/launch.sh
Path=$REPO
Icon=lagado
Terminal=false
Categories=Utility;
StartupNotify=true
StartupWMClass=Lagado
EOF
chmod +x "$DESKTOP"

# Refresh the launcher + icon caches so it appears immediately.
command -v update-desktop-database >/dev/null && update-desktop-database "$APPS_DIR" 2>/dev/null || true
command -v gtk-update-icon-cache  >/dev/null && gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true

echo "✅ Installed. Search 'Lagado' in your apps (the logo), click to launch, and right-click → pin to dock/favorites."
echo "   Desktop entry: $DESKTOP"
echo "   Icon:          $ICON"
