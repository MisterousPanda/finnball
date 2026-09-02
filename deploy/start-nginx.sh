#!/bin/sh
set -eu
PORT="${PORT:-8080}"
sed -i "s/LISTEN_PORT/${PORT}/g" /etc/nginx/conf.d/default.conf
exec nginx -g "daemon off;"
