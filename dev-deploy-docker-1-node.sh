#!/bin/sh

set -e

target/debug/crane -f ./crane.docker-1-node.toml deploy
