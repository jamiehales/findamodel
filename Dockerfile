# Stage 1: Build the React frontend
FROM node:24-alpine AS frontend-build
WORKDIR /app/frontend

COPY frontend/package.json frontend/yarn.lock* ./
# The ui-kit submodule is a `link:` dependency, so it must be present before installing.
COPY frontend/vendor/ui-kit ./vendor/ui-kit
RUN yarn install --frozen-lockfile

COPY frontend/ ./
RUN yarn build

# Stage 2: Build the Rust backend
FROM rust:1-trixie AS backend-build
WORKDIR /src

# Build dependencies against stub sources first so they are cached independently of
# application changes.
COPY Cargo.toml Cargo.lock ./
COPY backend/Cargo.toml backend/Cargo.toml
COPY desktop-tauri/src-tauri/Cargo.toml desktop-tauri/src-tauri/Cargo.toml
RUN mkdir -p backend/src desktop-tauri/src-tauri/src \
    && echo 'fn main() {}' > backend/src/main.rs \
    && touch backend/src/lib.rs \
    && echo 'fn main() {}' > desktop-tauri/src-tauri/src/main.rs \
    && cargo build --release --locked -p findamodel \
    && rm -rf backend/src

COPY backend/ backend/
RUN touch backend/src/lib.rs backend/src/main.rs \
    && cargo build --release --locked -p findamodel \
    && cp target/release/findamodel /findamodel

# Stage 3: Runtime image
FROM debian:trixie-slim
WORKDIR /app

# libvulkan1 and libegl1 load the host GPU driver (mounted by the NVIDIA container
# toolkit) for preview rendering; without a GPU the built-in CPU rasteriser is used.
RUN apt-get update && apt-get install -y --no-install-recommends \
    gosu \
    libegl1 \
    libvulkan1 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=backend-build /findamodel /app/findamodel
COPY --from=frontend-build /app/frontend/dist ./wwwroot
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

EXPOSE 8080
ENV FINDAMODEL_LISTEN=0.0.0.0:8080
ENV FINDAMODEL_DATA_PATH=/data
ENV FINDAMODEL_WEB_ROOT=/app/wwwroot
ENV PUID=1000
ENV PGID=1000
ENV UMASK=022
ENV NVIDIA_VISIBLE_DEVICES=all
ENV NVIDIA_DRIVER_CAPABILITIES=compute,utility,graphics

ENTRYPOINT ["/entrypoint.sh"]
