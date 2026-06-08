#!/bin/sh

if [ -z "$1" ]; then
    echo "Usage: $0 <major|minor|patch>"
    exit 1
fi

cargo release "$1" --execute
# --config release.toml
