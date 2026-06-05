#!/bin/sh

# Install cargo-nextest (faster test runner)
echo "=============================="
echo "Installing cargo-nextest"
echo "=============================="

# FROM https://github.com/nextest-rs/nextest/releases/tag/cargo-nextest-0.9.136
curl -LO https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-0.9.136/cargo-nextest-0.9.136-x86_64-unknown-linux-gnu.tar.gz

# Verify checksum
# sha256:a098eed56f2dd88c7fdca1e554a6b99fa1ffbd2a7a1c41b865700112981f6f52
echo "a098eed56f2dd88c7fdca1e554a6b99fa1ffbd2a7a1c41b865700112981f6f52  cargo-nextest-0.9.136-x86_64-unknown-linux-gnu.tar.gz" | sha256sum -c

# Extract to cargo bin dir
if [ ! -d "$HOME/.cargo/bin" ]; then
    mkdir -p ~/.cargo/bin
fi

tar -xzf cargo-nextest-0.9.136-x86_64-unknown-linux-gnu.tar.gz -C ~/.cargo/bin

rm cargo-nextest-0.9.136-x86_64-unknown-linux-gnu.tar.gz
