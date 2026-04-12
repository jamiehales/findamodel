#!/bin/sh
set -e

PUID=${PUID:-1000}
PGID=${PGID:-1000}

# LinuxServer compatibility: UMASK is primary, UMASK_SET is legacy fallback.
if [ -n "${UMASK:-}" ]; then
    EFFECTIVE_UMASK="$UMASK"
elif [ -n "${UMASK_SET:-}" ]; then
    EFFECTIVE_UMASK="$UMASK_SET"
else
    EFFECTIVE_UMASK="022"
fi

umask "$EFFECTIVE_UMASK"

# Ensure numeric uid/gid values to avoid confusing startup failures.
case "$PUID" in
    ''|*[!0-9]*)
        echo "Error: PUID must be numeric, got '$PUID'" >&2
        exit 1
        ;;
esac

case "$PGID" in
    ''|*[!0-9]*)
        echo "Error: PGID must be numeric, got '$PGID'" >&2
        exit 1
        ;;
esac

# Match LinuxServer-style behavior: run as root when explicitly requested.
if [ "$PUID" = "0" ] || [ "$PGID" = "0" ]; then
    exec dotnet findamodel.dll
fi

TARGET_GROUP="app"
TARGET_USER="app"

# Ensure the group exists at the requested gid.
if getent group "$PGID" > /dev/null 2>&1; then
    TARGET_GROUP="$(getent group "$PGID" | cut -d: -f1)"
elif getent group "$TARGET_GROUP" > /dev/null 2>&1; then
    groupmod -o -g "$PGID" "$TARGET_GROUP"
else
    groupadd -o -g "$PGID" "$TARGET_GROUP"
fi

# Ensure the user exists at the requested uid.
if getent passwd "$PUID" > /dev/null 2>&1; then
    TARGET_USER="$(getent passwd "$PUID" | cut -d: -f1)"
elif getent passwd "$TARGET_USER" > /dev/null 2>&1; then
    usermod -o -u "$PUID" -g "$PGID" "$TARGET_USER"
else
    useradd -o -u "$PUID" -g "$PGID" -M -N -s /usr/sbin/nologin "$TARGET_USER"
fi

exec gosu "$TARGET_USER:$TARGET_GROUP" dotnet findamodel.dll
