#!/bin/sh
set -e

echo "Generating documentation..."
cargo doc --no-deps

echo "Copying to doc/..."
rm -rf doc
cp -r target/doc doc

echo "Done! Documentation available at: ./doc/arcella/index.html"