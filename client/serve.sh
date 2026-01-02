#!/bin/bash
# Simple HTTP server for testing the client-side EPUB reader
# Requires Python 3

PORT=${1:-8080}
echo "Serving EPUB Reader at http://localhost:$PORT"
echo "Press Ctrl+C to stop"
python3 -m http.server $PORT
