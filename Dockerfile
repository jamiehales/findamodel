# Stage 1: Build the React frontend
FROM node:20-alpine AS frontend-build
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

COPY --from=backend-build /app/backend/out .
COPY --from=frontend-build /app/frontend/dist ./wwwroot

EXPOSE 80
ENV ASPNETCORE_URLS=http://+:80

ENTRYPOINT ["dotnet", "findamodel.dll"]
