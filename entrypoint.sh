#!/bin/sh
set -e

PUID=${PUID:-0}
PGID=${PGID:-0}
UMASK=${UMASK:-022}

umask "$UMASK"

if [ "$PUID" -eq 0 ] && [ "$PGID" -eq 0 ]; then
    exec dotnet findamodel.dll
fi

# Create group if it doesn't exist
if ! getent group "$PGID" > /dev/null 2>&1; then
    addgroup -g "$PGID" appgroup
fi

# Create user if it doesn't exist
if ! getent passwd "$PUID" > /dev/null 2>&1; then
    adduser -D -u "$PUID" -G "$(getent group "$PGID" | cut -d: -f1)" appuser
fi

exec gosu "$PUID:$PGID" dotnet findamodel.dll
