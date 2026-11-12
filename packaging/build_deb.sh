#!/bin/bash
set -euo pipefail

# Directory of this script
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_ROOT"

echo "=== [1/4] Компиляция релизной версии corporate-contacts ==="
cargo build --release --offline

BINARY="$PROJECT_ROOT/target/release/corporate-contacts"
if [ ! -f "$BINARY" ]; then
    echo "Ошибка: Бинарник не найден: $BINARY" >&2
    exit 1
fi

echo "=== [2/4] Подготовка файловой структуры пакета ==="
STAGING_DIR="$PROJECT_ROOT/packaging/staging"
rm -rf "$STAGING_DIR"

mkdir -p "$STAGING_DIR/usr/bin"
mkdir -p "$STAGING_DIR/usr/share/applications"
mkdir -p "$STAGING_DIR/usr/share/icons/hicolor/scalable/apps"
mkdir -p "$STAGING_DIR/usr/share/metainfo"
mkdir -p "$STAGING_DIR/usr/share/corporate-contacts"
mkdir -p "$STAGING_DIR/DEBIAN"

# Copy binary
install -m 755 "$BINARY" "$STAGING_DIR/usr/bin/corporate-contacts"

# Copy desktop launcher
install -m 644 "$PROJECT_ROOT/resources/org.corporate.contacts.desktop" "$STAGING_DIR/usr/share/applications/org.corporate.contacts.desktop"

# Copy SVG icon
install -m 644 "$PROJECT_ROOT/resources/org.corporate.contacts.svg" "$STAGING_DIR/usr/share/icons/hicolor/scalable/apps/org.corporate.contacts.svg"

# Copy AppStream metainfo
install -m 644 "$PROJECT_ROOT/resources/org.corporate.contacts.metainfo.xml" "$STAGING_DIR/usr/share/metainfo/org.corporate.contacts.metainfo.xml"
mkdir -p "$STAGING_DIR/usr/share/appdata"
install -m 644 "$PROJECT_ROOT/resources/org.corporate.contacts.metainfo.xml" "$STAGING_DIR/usr/share/appdata/org.corporate.contacts.appdata.xml"

# Copy default database (always guaranteed to be a clean empty list)
echo "[]" > "$STAGING_DIR/usr/share/corporate-contacts/data.json"
chmod 644 "$STAGING_DIR/usr/share/corporate-contacts/data.json"

# Copy Debian maintainer scripts
install -m 755 "$PROJECT_ROOT/packaging/debian/postinst" "$STAGING_DIR/DEBIAN/postinst"
install -m 755 "$PROJECT_ROOT/packaging/debian/postrm" "$STAGING_DIR/DEBIAN/postrm"
if [ -f "$PROJECT_ROOT/packaging/debian/prerm" ]; then
    install -m 755 "$PROJECT_ROOT/packaging/debian/prerm" "$STAGING_DIR/DEBIAN/prerm"
fi

# Prepare DEBIAN/control with Installed-Size
INSTALLED_SIZE=$(du -sk "$STAGING_DIR/usr" | awk '{print $1}')
sed "s/^Description:/Installed-Size: $INSTALLED_SIZE\nDescription:/" "$PROJECT_ROOT/packaging/debian/control" > "$STAGING_DIR/DEBIAN/control"
chmod 644 "$STAGING_DIR/DEBIAN/control"

# Set strict Debian directory permissions
find "$STAGING_DIR" -type d -exec chmod 755 {} +
chmod 755 "$STAGING_DIR/DEBIAN"

echo "=== [3/4] Сборка .deb пакета через dpkg-deb ==="
DEB_NAME="corporate-contacts_1.0.0_amd64.deb"
OUTPUT_DEB="$PROJECT_ROOT/$DEB_NAME"

dpkg-deb --build --root-owner-group "$STAGING_DIR" "$OUTPUT_DEB"
rm -rf "$STAGING_DIR"

echo "=== [4/4] Проверка собранного пакета ==="
echo "Информация о пакете:"
dpkg-deb -I "$OUTPUT_DEB"
echo ""
echo "Содержимое пакета:"
dpkg-deb -c "$OUTPUT_DEB"

echo ""
echo "✓ Пакет успешно собран: $OUTPUT_DEB"
echo "Для установки выполните: sudo dpkg -i $DEB_NAME"
