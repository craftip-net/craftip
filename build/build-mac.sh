#!/bin/bash

APP_NAME=CraftIP
if [ -z $X86_64_APPLE_DARWIN ]; then
    echo "X86_64_APPLE_DARWIN not set, using default."
    X86_64_APPLE_DARWIN=$(dirname "$0")/../target/x86_64-apple-darwin/release/client-gui
fi
if [ -z $AARCH64_APPLE_DARWIN ]; then
    echo "AARCH64_APPLE_DARWIN not set, using default."
    AARCH64_APPLE_DARWIN=$(dirname "$0")/../target/aarch64-apple-darwin/release/client-gui
fi
if [ -z $BUILD_FOLDER ]; then
    echo "BUILD_FOLDER not set, using default."
    BUILD_FOLDER=/tmp/mac-build
fi
if [ -z $DMG_OUTPUT_PATH ]; then
    echo "DMG_OUTPUT_PATH not set, using default."
    DMG_OUTPUT_PATH=$BUILD_FOLDER/CraftIP.dmg
fi
# on error, fail script
set -e

APP_DESTINATION=$BUILD_FOLDER/dmg/$APP_NAME.app
RESOURCES=$(dirname "$0")/resources
echo "cleaning up..."
rm -fr $BUILD_FOLDER/dmg
rm -fr DMG_OUTPUT_PATH
rm -fr $APP_DESTINATION
# creates all folders
mkdir -p $APP_DESTINATION

echo "Building $APP_NAME.app..."
mkdir -p $APP_DESTINATION/Contents/MacOS
mkdir -p $APP_DESTINATION/Contents/Resources
echo "building universal binary..."
lipo $X86_64_APPLE_DARWIN $AARCH64_APPLE_DARWIN -create -output $APP_DESTINATION/Contents/MacOS/CraftIP
chmod +x $APP_DESTINATION/Contents/MacOS/CraftIP
echo "copying resources..."
cp $RESOURCES/Info.plist $APP_DESTINATION/Contents/Info.plist

echo "building icon..."
ICON=$RESOURCES/logo-mac.png
ICON_BUILD=$APP_DESTINATION/Contents/Resources/logo
mkdir $ICON_BUILD.iconset
sips -z 16 16     $ICON --out $ICON_BUILD.iconset/icon_16x16.png
sips -z 32 32     $ICON --out $ICON_BUILD.iconset/icon_16x16@2x.png
sips -z 32 32     $ICON --out $ICON_BUILD.iconset/icon_32x32.png
sips -z 64 64     $ICON --out $ICON_BUILD.iconset/icon_32x32@2x.png
sips -z 128 128   $ICON --out $ICON_BUILD.iconset/icon_128x128.png
sips -z 256 256   $ICON --out $ICON_BUILD.iconset/icon_128x128@2x.png
sips -z 256 256   $ICON --out $ICON_BUILD.iconset/icon_256x256.png
sips -z 512 512   $ICON --out $ICON_BUILD.iconset/icon_256x256@2x.png
sips -z 512 512   $ICON --out $ICON_BUILD.iconset/icon_512x512.png
sips -z 1024 1024 $ICON --out $ICON_BUILD.iconset/icon_512x512@2x.png
iconutil -c icns $ICON_BUILD.iconset
echo "clean up..."
rm -r $ICON_BUILD.iconset

echo "building dmg..."
ln -s /Applications ${BUILD_FOLDER}/dmg/Applications
hdiutil create -volname "CraftIP" -srcfolder "${BUILD_FOLDER}/dmg" -ov -format UDZO "${DMG_OUTPUT_PATH}"


