#!/usr/bin/env bash
# Publish CogZ to the official MCP registry (registry.modelcontextprotocol.io).
# Stamps server.json version fields from the latest git tag — no manual bumping.
#
# Prerequisites: mcp-publisher on PATH, ghcr.io/balaianu/cogz pushed by the
# release workflow and set public, `mcp-publisher login github` done once.
set -euo pipefail
cd "$(dirname "$0")"

VER=$(git describe --tags --abbrev=0 | sed 's/^v//')
echo "Publishing io.github.balaianu/cogz v$VER"

sed -i -e "s/\"version\": \"[^\"]*\"/\"version\": \"$VER\"/g" \
       -e "s|ghcr.io/balaianu/cogz:[^\"]*|ghcr.io/balaianu/cogz:$VER|g" server.json

mcp-publisher validate
mcp-publisher publish
