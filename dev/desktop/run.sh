#!/bin/sh
# Builds and starts the test desktop. Open the printed URL in a browser; Ctrl+C stops it.
# The UltraViewer identity is kept in the uvview-test-config volume between runs.
set -e
cd "$(dirname "$0")/../.."
docker build -f dev/desktop/Dockerfile -t uvview-test .
docker run --rm -it -p 127.0.0.1:6080:6080 -v uvview-test-config:/root/.config uvview-test
