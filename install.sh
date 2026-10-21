#!/bin/sh
# Builds the release bundle and replaces the installed app in /Applications.
set -eu
cd "$(dirname "$0")"

# dx keeps old asset files in its app folder; start afresh so that only the
# current assets go into the bundle.
rm -rf target/dx/Timer/release/macos/Timer.app target/dx/Timer/bundle
dx bundle --package-types macos --release

osascript -e 'quit app "Timer"' 2>/dev/null || true
# The old app saves its session while quitting and holds the data file's
# lock until it is gone; the new one would otherwise start read-only.
# A Timer still running after ten seconds is another one (`dx serve`).
waited=0
while pgrep -xq Timer; do
    if [ "$waited" -ge 50 ]; then
        echo "Another Timer is still running; quit it and run install.sh again." >&2
        exit 1
    fi
    sleep 0.2
    waited=$((waited + 1))
done
rm -rf /Applications/Timer.app
cp -R target/dx/Timer/bundle/macos/macos/Timer.app /Applications/Timer.app
open /Applications/Timer.app

# The target folder grows to several GB and is not needed once the app is
# installed.
cargo clean
