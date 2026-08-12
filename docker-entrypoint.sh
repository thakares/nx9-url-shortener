#!/bin/sh
set -e

if [ "$1" = 'serve' ]; then
    /usr/local/bin/bzod init-admin
    exec /usr/local/bin/bzod serve
fi

exec /usr/local/bin/bzod "$@"
