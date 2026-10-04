#!/bin/bash
# Build the spike for the simulator, install it on a simulator and launch the host app.
# Usage: ios/spike/scripts/build-install.sh [SIMULATOR_UDID]   (default: booted)
set -euo pipefail
cd "$(dirname "$0")/.."
UDID="${1:-booted}"
DEST=$([ "$UDID" = booted ] && echo 'generic/platform=iOS Simulator' || echo "id=$UDID")
xcodebuild -project TakTak.xcodeproj -scheme TakTak -sdk iphonesimulator -configuration Debug \
  -derivedDataPath build -destination "$DEST" build | grep -E '^\*\* |error:|note: bundled'
APP=build/Build/Products/Debug-iphonesimulator/TakTak.app
xcrun simctl install "$UDID" "$APP"
xcrun simctl spawn "$UDID" pluginkit -m -v -i tech.taktak.ios.keyboard
xcrun simctl launch "$UDID" tech.taktak.ios
