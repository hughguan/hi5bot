#!/bin/sh
set -e

# Start Next.js standalone web server in background on port 3000 if node and web-standalone exist
if [ -d "/app/web-standalone" ] && command -v node >/dev/null 2>&1; then
    echo "[entrypoint] Starting Next.js Web Dashboard on port 3000..."
    PORT=3000 HOSTNAME=0.0.0.0 node /app/web-standalone/server.js &
fi

# Exec the main hi5bot Rust daemon process
echo "[entrypoint] Starting Hi5bot Rust daemon..."
exec /app/hi5bot "$@"
