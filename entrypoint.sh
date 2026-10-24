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

NVIDIA_EGL_VENDOR_DIR="/usr/share/glvnd/egl_vendor.d"
NVIDIA_EGL_VENDOR_FILE="$NVIDIA_EGL_VENDOR_DIR/10_nvidia.json"
NVIDIA_EGL_LIBRARY="/usr/lib/x86_64-linux-gnu/libEGL_nvidia.so.0"

NVIDIA_VULKAN_ICD_DIR="/usr/share/vulkan/icd.d"
NVIDIA_VULKAN_ICD_FILE="$NVIDIA_VULKAN_ICD_DIR/nvidia_icd.json"
NVIDIA_GLX_LIBRARY="/usr/lib/x86_64-linux-gnu/libGLX_nvidia.so.0"

# Some container runtimes mount the NVIDIA driver libraries without the matching Vulkan
# ICD or GLVND vendor JSON. Without them the GPU is invisible and previews fall back to
# the CPU rasteriser.
if [ -f "$NVIDIA_GLX_LIBRARY" ] && [ ! -f "$NVIDIA_VULKAN_ICD_FILE" ] && [ ! -f /etc/vulkan/icd.d/nvidia_icd.json ]; then
    mkdir -p "$NVIDIA_VULKAN_ICD_DIR"
    cat >"$NVIDIA_VULKAN_ICD_FILE" <<'EOF'
{
  "file_format_version": "1.0.0",
  "ICD": {
    "library_path": "libGLX_nvidia.so.0",
    "api_version": "1.3"
  }
}
EOF
fi

if [ -f "$NVIDIA_EGL_LIBRARY" ] && [ ! -f "$NVIDIA_EGL_VENDOR_FILE" ]; then
    mkdir -p "$NVIDIA_EGL_VENDOR_DIR"
    cat >"$NVIDIA_EGL_VENDOR_FILE" <<'EOF'
{
  "file_format_version": "1.0.0",
  "ICD": {
    "library_path": "libEGL_nvidia.so.0"
  }
}
EOF
fi

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
DATA_PATH="${FINDAMODEL_DATA_PATH:-/data}"
mkdir -p "$DATA_PATH"

if [ "$PUID" = "0" ] || [ "$PGID" = "0" ]; then
    exec /app/findamodel
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

# The data directory must be writable by the app user (the database and preview cache
# live there).
if ! gosu "$TARGET_USER:$TARGET_GROUP" test -w "$DATA_PATH"; then
    chown "$TARGET_USER:$TARGET_GROUP" "$DATA_PATH"
fi

exec gosu "$TARGET_USER:$TARGET_GROUP" /app/findamodel
