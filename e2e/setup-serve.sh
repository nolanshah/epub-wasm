#!/bin/sh
# Assemble the directory served to the e2e tests (symlinks into the repo).
set -e
cd "$(dirname "$0")"
rm -rf .serve
mkdir .serve
ln -s ../../client-test/index.html .serve/index.html
ln -s ../../client-test/rendition.html .serve/rendition.html
ln -s ../../client-test/pkg .serve/pkg
ln -s ../fixtures/fixture.epub .serve/fixture.epub
