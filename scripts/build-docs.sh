#!/bin/sh
set -e

echo "Generating documentation..."
cargo doc --no-deps

echo "Done! Documentation available at:
echo      ./target/doc/alme-proto/index.html"
echo      ./target/doc/arcella/index.html"
echo      ./target/doc/arcella-cli/index.html"