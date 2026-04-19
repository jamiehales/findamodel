using System.Buffers.Binary;
using System.Collections.Concurrent;
using findamodel.Models;

namespace findamodel.Services;

public sealed class AutoSupportSettingsPreviewService(
    AutoSupportGenerationV3Service autoSupportGenerationV3Service,
    ModelLoaderService modelLoaderService,
    MeshTransferService meshTransferService,
    IConfiguration config,
    ILoggerFactory loggerFactory)
{
    private static readonly TimeSpan PreviewRetention = TimeSpan.FromHours(1);

    private static readonly IReadOnlyList<ExternalStlScenarioDefinition> ExternalStlScenarios =
    [
        // Populate with files relative to Models:DirectoryPath.
        // Example: new ExternalStlScenarioDefinition("Mechanical test part", "preview/part-a.stl"),
    ];

    private readonly ILogger logger = loggerFactory.CreateLogger<AutoSupportSettingsPreviewService>();
    private readonly ConcurrentDictionary<Guid, PreviewCacheEntry> previews = new();
    private readonly string cacheDirectory = config["Cache:AutoSupportsPath"]
        ?? Path.Combine(Path.GetTempPath(), "findamodel", "auto-support");

    public async Task<AutoSupportSettingsPreviewDto> GeneratePreviewAsync(
        AutoSupportSettingsPreviewRequest request,
        CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        CleanupExpiredPreviews();

        ValidateRequest(request);

        Directory.CreateDirectory(cacheDirectory);
        var previewId = Guid.NewGuid();
        var tuning = ToTuningOverrides(request.Tuning);

        var scenarios = new List<PreviewScenario>
        {
            new("thin-plane-parallel", "Box 40x40x2mm (parallel)", "builtin", BuildParallelPlaneGeometry()),
            new("thin-plane-30deg", "Box 40x40x2mm (30 degrees)", "builtin", BuildAngledPlaneGeometry()),
            new("sphere-40", "Sphere 40mm diameter", "builtin", BuildSphereGeometry(20f, new Vec3(0f, 35f, 0f))),
            new("cube-40", "Cube 40mm", "builtin", BuildCubeGeometry(40f, new Vec3(0f, 30f, 0f))),
            new("cube-40-rotated-45", "Cube 40mm rotated 45 degrees", "builtin", BuildRotatedCubeGeometry(40f, MathF.PI / 4f, new Vec3(0f, 45f, 0f))),
            new("cone-upside-down", "Upside-down cone 40mm diameter x 80mm height", "builtin", BuildInvertedConeGeometry(20f, 80f, 10f)),
            new("donut-40", "Donut 40mm diameter (tests enclosed void)", "builtin", BuildDonutGeometry(15f, 5f, new Vec3(0f, 35f, 0f))),
        };

        await AppendExternalScenariosAsync(scenarios, cancellationToken);

        string? requestedScenarioId = null;
        if (!string.IsNullOrWhiteSpace(request.ScenarioId))
        {
            requestedScenarioId = request.ScenarioId.Trim();
            var exists = scenarios.Any(s => string.Equals(s.ScenarioId, requestedScenarioId, StringComparison.OrdinalIgnoreCase));
            if (!exists)
                throw new ArgumentException($"Unknown scenario id '{request.ScenarioId}'.");
        }

        var previewFiles = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        var scenarioDtos = new List<AutoSupportSettingsPreviewScenarioDto>(scenarios.Count);

        foreach (var scenario in scenarios)
        {
            cancellationToken.ThrowIfCancellationRequested();

            var shouldGenerate = requestedScenarioId == null
                || string.Equals(scenario.ScenarioId, requestedScenarioId, StringComparison.OrdinalIgnoreCase);

            if (!shouldGenerate)
            {
                scenarioDtos.Add(new AutoSupportSettingsPreviewScenarioDto(
                    scenario.ScenarioId,
                    scenario.Name,
                    scenario.Source,
                    "not-generated",
                    0,
                    null,
                    null,
                    null));
                continue;
            }

            try
            {
                var preview = autoSupportGenerationV3Service.GenerateSupportPreview(scenario.Geometry, tuning);
                var bodyPayload = meshTransferService.Encode(scenario.Geometry);
                var supportPayload = meshTransferService.Encode(preview.SupportGeometry);
                var envelope = BuildEnvelope(bodyPayload, supportPayload);

                var cacheFilePath = Path.Combine(cacheDirectory, $"{previewId:N}-{scenario.ScenarioId}.bin");
                await File.WriteAllBytesAsync(cacheFilePath, envelope, cancellationToken);
                previewFiles[scenario.ScenarioId] = cacheFilePath;

                scenarioDtos.Add(new AutoSupportSettingsPreviewScenarioDto(
                    scenario.ScenarioId,
                    scenario.Name,
                    scenario.Source,
                    "completed",
                    preview.SupportPoints.Count,
                    null,
                    [.. preview.SupportPoints.Select(point => new AutoSupportPointDto(
                        point.Position.X,
                        point.Position.Y,
                        point.Position.Z,
                        point.RadiusMm,
                        new AutoSupportVectorDto(point.PullForce.X, point.PullForce.Y, point.PullForce.Z),
                        point.Size.ToString().ToLowerInvariant()))],
                    [.. preview.Islands.Select(island => new AutoSupportIslandDto(
                        island.CentroidX,
                        island.CentroidZ,
                        island.SliceHeightMm,
                        island.AreaMm2,
                        island.RadiusMm))]));
            }
            catch (Exception ex)
            {
                logger.LogError(ex, "Failed to generate autosupport settings preview for scenario {ScenarioId}", scenario.ScenarioId);
                scenarioDtos.Add(new AutoSupportSettingsPreviewScenarioDto(
                    scenario.ScenarioId,
                    scenario.Name,
                    scenario.Source,
                    "failed",
                    0,
                    ex.Message,
                    [],
                    []));
            }
        }

        previews[previewId] = new PreviewCacheEntry(DateTime.UtcNow, previewFiles);

        return new AutoSupportSettingsPreviewDto(previewId, scenarioDtos);
    }

    public byte[]? GetScenarioEnvelope(Guid previewId, string scenarioId)
    {
        CleanupExpiredPreviews();

        if (!previews.TryGetValue(previewId, out var preview))
            return null;

        if (!preview.Files.TryGetValue(scenarioId, out var path))
            return null;

        if (!File.Exists(path))
            return null;

        return File.ReadAllBytes(path);
    }

    private async Task AppendExternalScenariosAsync(List<PreviewScenario> scenarios, CancellationToken cancellationToken)
    {
        if (ExternalStlScenarios.Count == 0)
            return;

        var modelsRoot = config["Models:DirectoryPath"];
        if (string.IsNullOrWhiteSpace(modelsRoot))
            return;

        foreach (var definition in ExternalStlScenarios)
        {
            cancellationToken.ThrowIfCancellationRequested();

            var fullPath = Path.Combine(modelsRoot, definition.RelativePath);
            if (!File.Exists(fullPath))
            {
                logger.LogWarning("Autosupport settings preview STL not found: {Path}", fullPath);
                continue;
            }

            var extension = Path.GetExtension(fullPath).TrimStart('.').ToLowerInvariant();
            var geometry = await modelLoaderService.LoadModelAsync(fullPath, extension);
            if (geometry == null)
            {
                logger.LogWarning("Autosupport settings preview STL failed to load: {Path}", fullPath);
                continue;
            }

            var scenarioId = $"stl-{SanitizeScenarioId(definition.Name)}";
            scenarios.Add(new PreviewScenario(
                scenarioId,
                definition.Name,
                "stl",
                geometry));
        }
    }

    private static string SanitizeScenarioId(string value)
    {
        var chars = value
            .Trim()
            .ToLowerInvariant()
            .Select(ch => char.IsLetterOrDigit(ch) ? ch : '-')
            .ToArray();

        var sanitized = new string(chars);
        while (sanitized.Contains("--", StringComparison.Ordinal))
            sanitized = sanitized.Replace("--", "-", StringComparison.Ordinal);

        return sanitized.Trim('-');
    }

    private static void ValidateRequest(AutoSupportSettingsPreviewRequest request)
    {
        ArgumentNullException.ThrowIfNull(request);
        ArgumentNullException.ThrowIfNull(request.Tuning);

        if (request.Tuning.MinVoxelSizeMm <= 0 || request.Tuning.MaxVoxelSizeMm < request.Tuning.MinVoxelSizeMm)
            throw new ArgumentException("Invalid voxel size range.");

        if (request.Tuning.MinLayerHeightMm <= 0 || request.Tuning.MaxLayerHeightMm < request.Tuning.MinLayerHeightMm)
            throw new ArgumentException("Invalid layer height range.");

    }

    private static AutoSupportV3TuningOverrides ToTuningOverrides(AutoSupportSettingsPreviewTuningRequest tuning)
        => new(
            tuning.BedMarginMm,
            tuning.MinVoxelSizeMm,
            tuning.MaxVoxelSizeMm,
            tuning.MinLayerHeightMm,
            tuning.MaxLayerHeightMm,
            tuning.MinIslandAreaMm2,
            tuning.MergeDistanceMm,
            tuning.ResinStrength,
            tuning.CrushForceThreshold,
            tuning.MaxAngularForce,
            tuning.PeelForceMultiplier,
            tuning.LightTipRadiusMm,
            tuning.MediumTipRadiusMm,
            tuning.HeavyTipRadiusMm,
            tuning.SuctionMultiplier,
            tuning.AreaGrowthThreshold,
            tuning.AreaGrowthMultiplier,
            tuning.GravityEnabled,
            tuning.ResinDensityGPerMl,
            tuning.DragCoefficientMultiplier,
            tuning.MinFeatureWidthMm,
            tuning.ShrinkagePercent,
            tuning.ShrinkageEdgeBias);

    private void CleanupExpiredPreviews()
    {
        var now = DateTime.UtcNow;
        var expired = previews
            .Where(pair => now - pair.Value.CreatedAtUtc > PreviewRetention)
            .Select(pair => pair.Key)
            .ToList();

        foreach (var previewId in expired)
        {
            if (!previews.TryRemove(previewId, out var removed))
                continue;

            foreach (var path in removed.Files.Values)
            {
                try
                {
                    if (File.Exists(path))
                        File.Delete(path);
                }
                catch
                {
                }
            }
        }
    }

    private static byte[] BuildEnvelope(byte[] bodyPayload, byte[] supportPayload)
    {
        var envelope = new byte[4 + bodyPayload.Length + 4 + supportPayload.Length];
        var span = envelope.AsSpan();
        BinaryPrimitives.WriteUInt32LittleEndian(span[0..4], (uint)bodyPayload.Length);
        bodyPayload.CopyTo(span[4..]);
        var afterBody = 4 + bodyPayload.Length;
        BinaryPrimitives.WriteUInt32LittleEndian(span[afterBody..(afterBody + 4)], (uint)supportPayload.Length);
        supportPayload.CopyTo(span[(afterBody + 4)..]);
        return envelope;
    }

    private static LoadedGeometry BuildParallelPlaneGeometry()
        => CreateGeometry([
            .. BuildBoxTrianglesList(
                new Vec3(-20f, 28f, -20f),
                new Vec3(20f, 30f, 20f))
        ]);

    private static LoadedGeometry BuildAngledPlaneGeometry()
    {
        var localPlane = BuildBoxTrianglesList(new Vec3(-20f, -1f, -20f), new Vec3(20f, 1f, 20f));
        var rotated = TransformTriangles(localPlane, point =>
        {
            var rotatedPoint = RotateX(point, MathF.PI / 6f);
            return Translate(rotatedPoint, 0f, 35f, 0f);
        });

        return CreateGeometry(rotated);
    }

    private static LoadedGeometry BuildSphereGeometry(float radiusMm, Vec3 centre)
    {
        const int latSegments = 24;
        const int lonSegments = 32;
        var triangles = new List<Triangle3D>(latSegments * lonSegments * 2);

        for (var lat = 0; lat < latSegments; lat++)
        {
            var theta0 = MathF.PI * lat / latSegments;
            var theta1 = MathF.PI * (lat + 1) / latSegments;

            for (var lon = 0; lon < lonSegments; lon++)
            {
                var phi0 = (MathF.PI * 2f * lon) / lonSegments;
                var phi1 = (MathF.PI * 2f * (lon + 1)) / lonSegments;

                var p00 = SpherePoint(centre, radiusMm, theta0, phi0);
                var p01 = SpherePoint(centre, radiusMm, theta0, phi1);
                var p10 = SpherePoint(centre, radiusMm, theta1, phi0);
                var p11 = SpherePoint(centre, radiusMm, theta1, phi1);

                if (lat > 0)
                    triangles.Add(MakeTriangle(p00, p10, p01));

                if (lat < latSegments - 1)
                    triangles.Add(MakeTriangle(p01, p10, p11));
            }
        }

        return CreateGeometry(triangles);
    }

    private static LoadedGeometry BuildCubeGeometry(float sizeMm, Vec3 centre)
    {
        var half = sizeMm / 2f;
        var min = new Vec3(centre.X - half, centre.Y - half, centre.Z - half);
        var max = new Vec3(centre.X + half, centre.Y + half, centre.Z + half);
        return CreateGeometry(BuildBoxTrianglesList(min, max));
    }

    private static LoadedGeometry BuildRotatedCubeGeometry(float sizeMm, float angleRad, Vec3 centre)
    {
        var half = sizeMm / 2f;
        var local = BuildBoxTrianglesList(new Vec3(-half, -half, -half), new Vec3(half, half, half));
        var rotated = TransformTriangles(local, point => Translate(RotateX(point, angleRad), centre.X, centre.Y, centre.Z));
        return CreateGeometry(rotated);
    }

    private static LoadedGeometry BuildInvertedConeGeometry(float radiusMm, float heightMm, float tipY)
    {
        const int segments = 48;
        var triangles = new List<Triangle3D>(segments * 2);

        var tip = new Vec3(0f, tipY, 0f);
        var baseY = tipY + heightMm;
        var baseCenter = new Vec3(0f, baseY, 0f);

        for (var i = 0; i < segments; i++)
        {
            var angle0 = (MathF.PI * 2f * i) / segments;
            var angle1 = (MathF.PI * 2f * (i + 1)) / segments;

            var p0 = new Vec3(radiusMm * MathF.Cos(angle0), baseY, radiusMm * MathF.Sin(angle0));
            var p1 = new Vec3(radiusMm * MathF.Cos(angle1), baseY, radiusMm * MathF.Sin(angle1));

            triangles.Add(MakeTriangle(tip, p1, p0));
            triangles.Add(MakeTriangle(baseCenter, p0, p1));
        }

        return CreateGeometry(triangles);
    }

    private static LoadedGeometry BuildDonutGeometry(float majorRadiusMm, float minorRadiusMm, Vec3 centre)
    {
        const int majorSegments = 32;
        const int minorSegments = 24;
        var triangles = new List<Triangle3D>(majorSegments * minorSegments * 2);

        for (var i = 0; i < majorSegments; i++)
        {
            var u0 = (MathF.PI * 2f * i) / majorSegments;
            var u1 = (MathF.PI * 2f * (i + 1)) / majorSegments;

            for (var j = 0; j < minorSegments; j++)
            {
                var v0 = (MathF.PI * 2f * j) / minorSegments;
                var v1 = (MathF.PI * 2f * (j + 1)) / minorSegments;

                var p00 = TorusPoint(centre, majorRadiusMm, minorRadiusMm, u0, v0);
                var p01 = TorusPoint(centre, majorRadiusMm, minorRadiusMm, u0, v1);
                var p10 = TorusPoint(centre, majorRadiusMm, minorRadiusMm, u1, v0);
                var p11 = TorusPoint(centre, majorRadiusMm, minorRadiusMm, u1, v1);

                triangles.Add(MakeTriangle(p00, p10, p01));
                triangles.Add(MakeTriangle(p01, p10, p11));
            }
        }

        return CreateGeometry(triangles);
    }

    private static LoadedGeometry BuildBoxTriangles(Vec3 min, Vec3 max)
    {
        return CreateGeometry(BuildBoxTrianglesList(min, max));
    }

    private static List<Triangle3D> BuildBoxTrianglesList(Vec3 min, Vec3 max)
    {
        var triangles = new List<Triangle3D>(12);

        var p000 = new Vec3(min.X, min.Y, min.Z);
        var p001 = new Vec3(min.X, min.Y, max.Z);
        var p010 = new Vec3(min.X, max.Y, min.Z);
        var p011 = new Vec3(min.X, max.Y, max.Z);
        var p100 = new Vec3(max.X, min.Y, min.Z);
        var p101 = new Vec3(max.X, min.Y, max.Z);
        var p110 = new Vec3(max.X, max.Y, min.Z);
        var p111 = new Vec3(max.X, max.Y, max.Z);

        AddQuad(triangles, p000, p100, p110, p010);
        AddQuad(triangles, p101, p001, p011, p111);
        AddQuad(triangles, p001, p000, p010, p011);
        AddQuad(triangles, p100, p101, p111, p110);
        AddQuad(triangles, p010, p110, p111, p011);
        AddQuad(triangles, p001, p101, p100, p000);

        return triangles;
    }

    private static void AddQuad(List<Triangle3D> triangles, Vec3 a, Vec3 b, Vec3 c, Vec3 d)
    {
        triangles.Add(MakeTriangle(a, b, c));
        triangles.Add(MakeTriangle(a, c, d));
    }

    private static Triangle3D MakeTriangle(Vec3 a, Vec3 b, Vec3 c)
        => new(a, c, b, (c - a).Cross(b - a).Normalized);

    private static List<Triangle3D> TransformTriangles(List<Triangle3D> triangles, Func<Vec3, Vec3> transform)
    {
        var transformed = new List<Triangle3D>(triangles.Count);
        foreach (var triangle in triangles)
        {
            var v0 = transform(triangle.V0);
            var v1 = transform(triangle.V1);
            var v2 = transform(triangle.V2);
            transformed.Add(MakeTriangle(v0, v1, v2));
        }

        return transformed;
    }

    private static LoadedGeometry CreateGeometry(List<Triangle3D> triangles)
    {
        if (triangles.Count == 0)
        {
            return new LoadedGeometry
            {
                Triangles = triangles,
                DimensionXMm = 0f,
                DimensionYMm = 0f,
                DimensionZMm = 0f,
                SphereCentre = new Vec3(0f, 0f, 0f),
                SphereRadius = 0f,
            };
        }

        float minX = float.MaxValue;
        float minY = float.MaxValue;
        float minZ = float.MaxValue;
        float maxX = float.MinValue;
        float maxY = float.MinValue;
        float maxZ = float.MinValue;

        foreach (var triangle in triangles)
        {
            UpdateBounds(triangle.V0, ref minX, ref minY, ref minZ, ref maxX, ref maxY, ref maxZ);
            UpdateBounds(triangle.V1, ref minX, ref minY, ref minZ, ref maxX, ref maxY, ref maxZ);
            UpdateBounds(triangle.V2, ref minX, ref minY, ref minZ, ref maxX, ref maxY, ref maxZ);
        }

        var offsetX = -((minX + maxX) * 0.5f);
        var offsetY = -minY;
        var offsetZ = -((minZ + maxZ) * 0.5f);

        if (MathF.Abs(offsetX) > 0.0001f || MathF.Abs(offsetY) > 0.0001f || MathF.Abs(offsetZ) > 0.0001f)
        {
            var normalized = new List<Triangle3D>(triangles.Count);
            foreach (var triangle in triangles)
            {
                var v0 = Translate(triangle.V0, offsetX, offsetY, offsetZ);
                var v1 = Translate(triangle.V1, offsetX, offsetY, offsetZ);
                var v2 = Translate(triangle.V2, offsetX, offsetY, offsetZ);
                normalized.Add(MakeTriangle(v0, v1, v2));
            }

            triangles = normalized;

            minX = float.MaxValue;
            minY = float.MaxValue;
            minZ = float.MaxValue;
            maxX = float.MinValue;
            maxY = float.MinValue;
            maxZ = float.MinValue;

            foreach (var triangle in triangles)
            {
                UpdateBounds(triangle.V0, ref minX, ref minY, ref minZ, ref maxX, ref maxY, ref maxZ);
                UpdateBounds(triangle.V1, ref minX, ref minY, ref minZ, ref maxX, ref maxY, ref maxZ);
                UpdateBounds(triangle.V2, ref minX, ref minY, ref minZ, ref maxX, ref maxY, ref maxZ);
            }
        }

        var dimensionX = maxX - minX;
        var dimensionY = maxY - minY;
        var dimensionZ = maxZ - minZ;

        var sphereCentre = new Vec3((minX + maxX) * 0.5f, (minY + maxY) * 0.5f, (minZ + maxZ) * 0.5f);
        var sphereRadius = 0f;
        foreach (var triangle in triangles)
        {
            sphereRadius = MathF.Max(sphereRadius, (triangle.V0 - sphereCentre).Length);
            sphereRadius = MathF.Max(sphereRadius, (triangle.V1 - sphereCentre).Length);
            sphereRadius = MathF.Max(sphereRadius, (triangle.V2 - sphereCentre).Length);
        }

        return new LoadedGeometry
        {
            Triangles = triangles,
            DimensionXMm = dimensionX,
            DimensionYMm = dimensionY,
            DimensionZMm = dimensionZ,
            SphereCentre = sphereCentre,
            SphereRadius = sphereRadius,
        };
    }

    private static void UpdateBounds(
        Vec3 point,
        ref float minX,
        ref float minY,
        ref float minZ,
        ref float maxX,
        ref float maxY,
        ref float maxZ)
    {
        minX = MathF.Min(minX, point.X);
        minY = MathF.Min(minY, point.Y);
        minZ = MathF.Min(minZ, point.Z);
        maxX = MathF.Max(maxX, point.X);
        maxY = MathF.Max(maxY, point.Y);
        maxZ = MathF.Max(maxZ, point.Z);
    }

    private static Vec3 SpherePoint(Vec3 centre, float radiusMm, float theta, float phi)
    {
        var sinTheta = MathF.Sin(theta);
        return new Vec3(
            centre.X + (radiusMm * sinTheta * MathF.Cos(phi)),
            centre.Y + (radiusMm * MathF.Cos(theta)),
            centre.Z + (radiusMm * sinTheta * MathF.Sin(phi)));
    }

    private static Vec3 TorusPoint(Vec3 centre, float majorRadiusMm, float minorRadiusMm, float u, float v)
    {
        var cosU = MathF.Cos(u);
        var sinU = MathF.Sin(u);
        var cosV = MathF.Cos(v);
        var sinV = MathF.Sin(v);

        var x = (majorRadiusMm + (minorRadiusMm * cosV)) * cosU;
        var y = minorRadiusMm * sinV;
        var z = (majorRadiusMm + (minorRadiusMm * cosV)) * sinU;

        return new Vec3(
            centre.X + x,
            centre.Y + y,
            centre.Z + z);
    }

    private static Vec3 RotateX(Vec3 point, float angleRad)
    {
        var cos = MathF.Cos(angleRad);
        var sin = MathF.Sin(angleRad);
        return new Vec3(
            point.X,
            (point.Y * cos) - (point.Z * sin),
            (point.Y * sin) + (point.Z * cos));
    }

    private static Vec3 Translate(Vec3 point, float x, float y, float z)
        => new(point.X + x, point.Y + y, point.Z + z);

    private sealed record PreviewScenario(string ScenarioId, string Name, string Source, LoadedGeometry Geometry);

    private sealed record PreviewCacheEntry(DateTime CreatedAtUtc, IReadOnlyDictionary<string, string> Files);

    private sealed record ExternalStlScenarioDefinition(string Name, string RelativePath);
}
