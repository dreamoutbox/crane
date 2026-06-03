#!/bin/sh

cd demo
go build
cd ..

cargo run -- -f demo/crane.toml deploy
