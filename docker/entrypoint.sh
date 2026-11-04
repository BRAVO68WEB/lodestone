#!/bin/bash
set -euo pipefail

data="${LODESTONE_PATH:-/var/lib/lodestone}"
mkdir -p "$data"
# CIFS/SMB often rejects chown. The share should be mounted as uid 1000.
if ! chown -R lodestone:lodestone "$data"; then
    echo "lodestone: could not change ownership of $data. Continuing." >&2
fi
cp /etc/lodestone/config.js /usr/share/nginx/html/config.js

gosu lodestone /usr/local/bin/lodestone_core &
core_pid=$!

nginx -g 'daemon off;' &
nginx_pid=$!

shutdown() {
    kill -TERM "$core_pid" "$nginx_pid" 2>/dev/null || true
    wait "$core_pid" "$nginx_pid" 2>/dev/null || true
}
trap shutdown TERM INT

set +e
wait -n "$core_pid" "$nginx_pid"
status=$?
set -e
shutdown
exit "$status"
