#!/bin/bash
# SayType Icon Generator - macOS 26 Style
# 套用圓角效果並生成各平台所需尺寸

set -e

SOURCE_LOGO="/Volumes/Data_1T/UserData/Downloads/Handy-main/docs/saytype/assets/logo.png"
ICONS_DIR="$(dirname "$0")/../src-tauri/icons"
TEMP_DIR="/tmp/saytype-icons"

# 清理並建立暫存目錄
rm -rf "$TEMP_DIR"
mkdir -p "$TEMP_DIR"

echo "=== SayType Icon Generator ==="
echo "來源: $SOURCE_LOGO"
echo "目標: $ICONS_DIR"

# 計算 macOS 26 風格圓角（約 22% 的圓角比例）
apply_rounded_corners() {
    local input="$1"
    local output="$2"
    local size="$3"
    local radius=$(echo "$size * 0.22" | bc | cut -d'.' -f1)

    # 先縮放到目標尺寸
    magick "$input" -resize "${size}x${size}" "$TEMP_DIR/resized.png"

    # 建立圓角遮罩
    magick -size "${size}x${size}" xc:none \
        -draw "roundrectangle 0,0,$((size-1)),$((size-1)),$radius,$radius" \
        "$TEMP_DIR/mask.png"

    # 套用遮罩
    magick "$TEMP_DIR/resized.png" "$TEMP_DIR/mask.png" \
        -compose DstIn -composite "$output"

    echo "  生成: $output (${size}x${size}, 圓角半徑: ${radius}px)"
}

echo ""
echo "1. 生成基本 PNG 尺寸..."

# 標準尺寸
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/32x32.png" 32
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/64x64.png" 64
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/128x128.png" 128
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/128x128@2x.png" 256
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/icon.png" 512
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/logo.png" 512

echo ""
echo "2. 生成 Windows Square Logo 系列..."

# Windows Square Logos
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/Square30x30Logo.png" 30
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/Square44x44Logo.png" 44
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/Square71x71Logo.png" 71
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/Square89x89Logo.png" 89
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/Square107x107Logo.png" 107
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/Square142x142Logo.png" 142
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/Square150x150Logo.png" 150
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/Square284x284Logo.png" 284
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/Square310x310Logo.png" 310
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/StoreLogo.png" 50

echo ""
echo "3. 生成 macOS .icns 檔案..."

# 建立 iconset 目錄
ICONSET_DIR="$TEMP_DIR/icon.iconset"
mkdir -p "$ICONSET_DIR"

# macOS iconset 需要的尺寸
apply_rounded_corners "$SOURCE_LOGO" "$ICONSET_DIR/icon_16x16.png" 16
apply_rounded_corners "$SOURCE_LOGO" "$ICONSET_DIR/icon_16x16@2x.png" 32
apply_rounded_corners "$SOURCE_LOGO" "$ICONSET_DIR/icon_32x32.png" 32
apply_rounded_corners "$SOURCE_LOGO" "$ICONSET_DIR/icon_32x32@2x.png" 64
apply_rounded_corners "$SOURCE_LOGO" "$ICONSET_DIR/icon_128x128.png" 128
apply_rounded_corners "$SOURCE_LOGO" "$ICONSET_DIR/icon_128x128@2x.png" 256
apply_rounded_corners "$SOURCE_LOGO" "$ICONSET_DIR/icon_256x256.png" 256
apply_rounded_corners "$SOURCE_LOGO" "$ICONSET_DIR/icon_256x256@2x.png" 512
apply_rounded_corners "$SOURCE_LOGO" "$ICONSET_DIR/icon_512x512.png" 512
apply_rounded_corners "$SOURCE_LOGO" "$ICONSET_DIR/icon_512x512@2x.png" 1024

# 使用 iconutil 生成 .icns
iconutil -c icns "$ICONSET_DIR" -o "$ICONS_DIR/icon.icns"
echo "  生成: $ICONS_DIR/icon.icns"

echo ""
echo "4. 生成 Windows .ico 檔案..."

# Windows ICO（包含多個尺寸）
magick "$ICONS_DIR/16x16.png" 2>/dev/null || apply_rounded_corners "$SOURCE_LOGO" "$TEMP_DIR/16.png" 16
magick "$TEMP_DIR/16.png" \
    "$ICONS_DIR/32x32.png" \
    "$ICONS_DIR/64x64.png" \
    "$ICONS_DIR/128x128.png" \
    "$ICONS_DIR/128x128@2x.png" \
    "$ICONS_DIR/icon.ico"
echo "  生成: $ICONS_DIR/icon.ico"

echo ""
echo "5. 更新 iOS icons..."

mkdir -p "$ICONS_DIR/ios"
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/ios/AppIcon-20x20@1x.png" 20
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/ios/AppIcon-20x20@2x.png" 40
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/ios/AppIcon-20x20@3x.png" 60
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/ios/AppIcon-29x29@1x.png" 29
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/ios/AppIcon-29x29@2x.png" 58
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/ios/AppIcon-29x29@3x.png" 87
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/ios/AppIcon-40x40@1x.png" 40
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/ios/AppIcon-40x40@2x.png" 80
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/ios/AppIcon-40x40@3x.png" 120
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/ios/AppIcon-60x60@2x.png" 120
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/ios/AppIcon-60x60@3x.png" 180
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/ios/AppIcon-76x76@1x.png" 76
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/ios/AppIcon-76x76@2x.png" 152
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/ios/AppIcon-83.5x83.5@2x.png" 167
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/ios/AppIcon-512@2x.png" 1024

echo ""
echo "6. 更新 Android icons..."

mkdir -p "$ICONS_DIR/android"
# Android 使用不同的圓角比例，但為一致性我們使用相同處理
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/android/mipmap-hdpi/ic_launcher.png" 72
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/android/mipmap-hdpi/ic_launcher_round.png" 72
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/android/mipmap-mdpi/ic_launcher.png" 48
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/android/mipmap-mdpi/ic_launcher_round.png" 48
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/android/mipmap-xhdpi/ic_launcher.png" 96
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/android/mipmap-xhdpi/ic_launcher_round.png" 96
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/android/mipmap-xxhdpi/ic_launcher.png" 144
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/android/mipmap-xxhdpi/ic_launcher_round.png" 144
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/android/mipmap-xxxhdpi/ic_launcher.png" 192
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/android/mipmap-xxxhdpi/ic_launcher_round.png" 192

# Android foreground/background 層
mkdir -p "$ICONS_DIR/android/mipmap-hdpi" "$ICONS_DIR/android/mipmap-mdpi" "$ICONS_DIR/android/mipmap-xhdpi" "$ICONS_DIR/android/mipmap-xxhdpi" "$ICONS_DIR/android/mipmap-xxxhdpi"
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/android/mipmap-hdpi/ic_launcher_foreground.png" 162
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/android/mipmap-mdpi/ic_launcher_foreground.png" 108
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/android/mipmap-xhdpi/ic_launcher_foreground.png" 216
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/android/mipmap-xxhdpi/ic_launcher_foreground.png" 324
apply_rounded_corners "$SOURCE_LOGO" "$ICONS_DIR/android/mipmap-xxxhdpi/ic_launcher_foreground.png" 432

echo ""
echo "=== 完成！==="

# 清理
rm -rf "$TEMP_DIR"

echo "所有 icon 已生成完畢。"
