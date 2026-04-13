# Stage 1: Build the React frontend
FROM node:24-alpine AS frontend-build
WORKDIR /app/frontend

COPY frontend/package.json frontend/yarn.lock* ./
RUN yarn install --frozen-lockfile

COPY frontend/ ./
RUN yarn build

# Stage 2: Build the ASP.NET backend
FROM mcr.microsoft.com/dotnet/sdk:9.0 AS backend-build
WORKDIR /app/backend

COPY backend/*.csproj ./
RUN dotnet restore

COPY backend/ ./
RUN dotnet publish -c Release -o out

# Stage 3: Final runtime image
FROM mcr.microsoft.com/dotnet/aspnet:9.0
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    gosu \
    libgl1-mesa-glx \
    libegl1-mesa \
    libgles2-mesa \
    libvulkan1 \
    libgomp1 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=backend-build /app/backend/out .
COPY --from=frontend-build /app/frontend/dist ./wwwroot
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

EXPOSE 80
ENV ASPNETCORE_URLS=http://+:80
ENV PUID=1000
ENV PGID=1000
ENV UMASK=022
ENV NVIDIA_VISIBLE_DEVICES=all
ENV NVIDIA_DRIVER_CAPABILITIES=compute,utility

ENTRYPOINT ["/entrypoint.sh"]
