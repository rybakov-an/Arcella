#!/bin/sh
set -e

echo "Generating documentation..."
cargo doc --no-deps

echo "Done! Documentation available at: ./target/doc/arcella/index.html"