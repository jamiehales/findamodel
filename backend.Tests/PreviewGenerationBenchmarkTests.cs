using System.Diagnostics;
using findamodel.Services;
using Microsoft.Data.Sqlite;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;
using Xunit.Abstractions;

namespace findamodel.Tests;

public class PreviewGenerationBenchmarkTests(ITestOutputHelper output)
{
    [Fact]
    public async Task Benchmark_GpuDualVariant_CurrentVsSharedBodyBuffer()
    {
        if (!string.Equals(Environment.GetEnvironmentVariable("FINDAMODEL_RUN_GPU_BENCHMARKS"), "1", StringComparison.Ordinal))
            return;

        var geometry = CreateBenchmarkGeometry();
        var (bodyTriangles, supportTriangles) = new SupportSeparationService().Separate(geometry.Triangles);
        var bodyColor = new System.Numerics.Vector3(0x81 / 255f, 0x8c / 255f, 0xf8 / 255f);
        var supportColor = new System.Numerics.Vector3(0xf5 / 255f, 0x9e / 255f, 0x0b / 255f);

        using var glContext = new GlPreviewContext(NullLoggerFactory.Instance);
        await glContext.WaitReadyAsync();

        if (!glContext.IsAvailable)
        {
            output.WriteLine("gpu benchmark skipped: GL context unavailable");
            return;
        }

        await glContext.RenderVariantPairCurrentAsync(bodyTriangles, supportTriangles, 512, 512, bodyColor, supportColor);
        await glContext.RenderVariantPairSharedBodyBufferAsync(bodyTriangles, supportTriangles, 512, 512, bodyColor, supportColor);

        const int iterations = 10;
        var currentSamples = await MeasureAsync(iterations, _ =>
            glContext.RenderVariantPairCurrentAsync(bodyTriangles, supportTriangles, 512, 512, bodyColor, supportColor));
        var sharedSamples = await MeasureAsync(iterations, _ =>
            glContext.RenderVariantPairSharedBodyBufferAsync(bodyTriangles, supportTriangles, 512, 512, bodyColor, supportColor));

        var currentAverageMs = currentSamples.Average();
        var sharedAverageMs = sharedSamples.Average();
        var deltaMs = sharedAverageMs - currentAverageMs;
        var deltaPercent = currentAverageMs <= 0d ? 0d : deltaMs / currentAverageMs * 100d;

        output.WriteLine($"gpu renderer: {glContext.RendererName}");
        output.WriteLine($"gpu benchmark body triangles: {bodyTriangles.Count}");
        output.WriteLine($"gpu benchmark support triangles: {supportTriangles?.Count ?? 0}");
        output.WriteLine($"gpu current dual-upload avg ms: {currentAverageMs:F2}");
        output.WriteLine($"gpu shared-body-buffer avg ms: {sharedAverageMs:F2}");
        output.WriteLine($"gpu shared-body-buffer delta: {deltaMs:F2} ms ({deltaPercent:F1}%)");
    }

    [Fact]
    public async Task Benchmark_GpuDualVariant_RealSupportedModel_CurrentVsSharedBodyBuffer()
    {
        if (!string.Equals(Environment.GetEnvironmentVariable("FINDAMODEL_RUN_GPU_BENCHMARKS"), "1", StringComparison.Ordinal))
            return;

        var sample = await TryLoadRealSupportedGeometryAsync();
        if (sample == null)
        {
            output.WriteLine("gpu real-model benchmark skipped: no configured supported model file was found");
            return;
        }

        var (bodyTriangles, supportTriangles) = new SupportSeparationService().Separate(sample.Geometry.Triangles);
        var bodyColor = new System.Numerics.Vector3(0x81 / 255f, 0x8c / 255f, 0xf8 / 255f);
        var supportColor = new System.Numerics.Vector3(0xf5 / 255f, 0x9e / 255f, 0x0b / 255f);

        using var glContext = new GlPreviewContext(NullLoggerFactory.Instance);
        await glContext.WaitReadyAsync();

        if (!glContext.IsAvailable)
        {
            output.WriteLine("gpu real-model benchmark skipped: GL context unavailable");
            return;
        }

        await glContext.RenderVariantPairCurrentAsync(bodyTriangles, supportTriangles, 512, 512, bodyColor, supportColor);
        await glContext.RenderVariantPairSharedBodyBufferAsync(bodyTriangles, supportTriangles, 512, 512, bodyColor, supportColor);

        const int iterations = 10;
        var currentSamples = await MeasureAsync(iterations, _ =>
            glContext.RenderVariantPairCurrentAsync(bodyTriangles, supportTriangles, 512, 512, bodyColor, supportColor));
        var sharedSamples = await MeasureAsync(iterations, _ =>
            glContext.RenderVariantPairSharedBodyBufferAsync(bodyTriangles, supportTriangles, 512, 512, bodyColor, supportColor));

        var currentAverageMs = currentSamples.Average();
        var sharedAverageMs = sharedSamples.Average();
        var deltaMs = sharedAverageMs - currentAverageMs;
        var deltaPercent = currentAverageMs <= 0d ? 0d : deltaMs / currentAverageMs * 100d;

        output.WriteLine($"gpu real-model benchmark file: {sample.FilePath}");
        output.WriteLine($"gpu real-model renderer: {glContext.RendererName}");
        output.WriteLine($"gpu real-model original triangles: {sample.Geometry.Triangles.Count}");
        output.WriteLine($"gpu real-model body triangles: {bodyTriangles.Count}");
        output.WriteLine($"gpu real-model support triangles: {supportTriangles?.Count ?? 0}");
        output.WriteLine($"gpu real-model current dual-upload avg ms: {currentAverageMs:F2}");
        output.WriteLine($"gpu real-model shared-body-buffer avg ms: {sharedAverageMs:F2}");
        output.WriteLine($"gpu real-model shared-body-buffer delta: {deltaMs:F2} ms ({deltaPercent:F1}%)");
    }

    [Fact]
    public async Task Benchmark_SingleVariantVersusDualVariant()
    {
        if (!string.Equals(Environment.GetEnvironmentVariable("FINDAMODEL_RUN_BENCHMARKS"), "1", StringComparison.Ordinal))
            return;

        var configuration = new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Preview:UseGpu"] = "false",
            })
            .Build();

        var geometry = CreateBenchmarkGeometry();
        var tempRoot = Path.Combine(Path.GetTempPath(), $"findamodel-preview-bench-{Guid.NewGuid():N}");
        Directory.CreateDirectory(tempRoot);

        using var glContext = new GlPreviewContext(NullLoggerFactory.Instance);
        var service = new ModelPreviewService(
            loaderService: null!,
            glContext,
            new SupportSeparationService(),
            configuration,
            NullLoggerFactory.Instance);
        service.SetCacheDirectory(tempRoot);

        try
        {
            await service.GeneratePreviewWithStatusAsync(geometry, $"warmup-{Guid.NewGuid():N}", includeSupports: true);
            await service.GeneratePreviewVariantsWithStatusAsync(geometry, $"warmup-both-{Guid.NewGuid():N}", true, true);

            const int iterations = 5;
            var includeSupportsSamples = await MeasureAsync(iterations, hash =>
                service.GeneratePreviewWithStatusAsync(geometry, hash, includeSupports: true));
            var excludeSupportsSamples = await MeasureAsync(iterations, hash =>
                service.GeneratePreviewWithStatusAsync(geometry, hash, includeSupports: false));
            var dualSamples = await MeasureAsync(iterations, hash =>
                service.GeneratePreviewVariantsWithStatusAsync(geometry, hash, true, true));

            var includeAverageMs = includeSupportsSamples.Average();
            var excludeAverageMs = excludeSupportsSamples.Average();
            var dualAverageMs = dualSamples.Average();
            var includeDeltaMs = dualAverageMs - includeAverageMs;
            var excludeDeltaMs = dualAverageMs - excludeAverageMs;
            var includeDeltaPercent = includeAverageMs <= 0d ? 0d : includeDeltaMs / includeAverageMs * 100d;
            var excludeDeltaPercent = excludeAverageMs <= 0d ? 0d : excludeDeltaMs / excludeAverageMs * 100d;

            output.WriteLine($"preview benchmark geometry triangles: {geometry.Triangles.Count}");
            output.WriteLine($"single include-supports avg ms: {includeAverageMs:F2}");
            output.WriteLine($"single exclude-supports avg ms: {excludeAverageMs:F2}");
            output.WriteLine($"dual variant avg ms: {dualAverageMs:F2}");
            output.WriteLine($"dual delta vs include-supports: {includeDeltaMs:F2} ms ({includeDeltaPercent:F1}%)");
            output.WriteLine($"dual delta vs exclude-supports: {excludeDeltaMs:F2} ms ({excludeDeltaPercent:F1}%)");
        }
        finally
        {
            Directory.Delete(tempRoot, recursive: true);
        }
    }

    private static async Task<List<double>> MeasureAsync(int iterations, Func<string, Task> runAsync)
    {
        var samples = new List<double>(iterations);
        for (var index = 0; index < iterations; index++)
        {
            var sw = Stopwatch.StartNew();
            await runAsync($"bench-{index}-{Guid.NewGuid():N}");
            sw.Stop();
            samples.Add(sw.Elapsed.TotalMilliseconds);
        }

        return samples;
    }

    private static LoadedGeometry CreateBenchmarkGeometry()
    {
        var triangles = new List<Triangle3D>(16000);

        for (var x = 0; x < 28; x++)
        {
            for (var z = 0; z < 28; z++)
            {
                AppendCuboid(
                    triangles,
                    min: new Vec3(x, 0f, z),
                    max: new Vec3(x + 1f, 1f, z + 1f));
            }
        }

        for (var x = 0; x < 12; x++)
        {
            for (var z = 0; z < 12; z++)
            {
                AppendCuboid(
                    triangles,
                    min: new Vec3(40f + x * 1.1f, 0f, z * 1.1f),
                    max: new Vec3(40f + x * 1.1f + 0.3f, 2.2f, z * 1.1f + 0.3f));
            }
        }

        return new LoadedGeometry
        {
            Triangles = triangles,
            SphereCentre = new Vec3(26f, 1.1f, 14f),
            SphereRadius = 32f,
            DimensionXMm = 54f,
            DimensionYMm = 2.2f,
            DimensionZMm = 28f,
        };
    }

    private static void AppendCuboid(List<Triangle3D> triangles, Vec3 min, Vec3 max)
    {
        var p000 = new Vec3(min.X, min.Y, min.Z);
        var p001 = new Vec3(min.X, min.Y, max.Z);
        var p010 = new Vec3(min.X, max.Y, min.Z);
        var p011 = new Vec3(min.X, max.Y, max.Z);
        var p100 = new Vec3(max.X, min.Y, min.Z);
        var p101 = new Vec3(max.X, min.Y, max.Z);
        var p110 = new Vec3(max.X, max.Y, min.Z);
        var p111 = new Vec3(max.X, max.Y, max.Z);

        AddFace(triangles, p000, p100, p110, p010, new Vec3(0f, 0f, -1f));
        AddFace(triangles, p101, p001, p011, p111, new Vec3(0f, 0f, 1f));
        AddFace(triangles, p001, p000, p010, p011, new Vec3(-1f, 0f, 0f));
        AddFace(triangles, p100, p101, p111, p110, new Vec3(1f, 0f, 0f));
        AddFace(triangles, p010, p110, p111, p011, new Vec3(0f, 1f, 0f));
        AddFace(triangles, p001, p101, p100, p000, new Vec3(0f, -1f, 0f));
    }

    private static void AddFace(List<Triangle3D> triangles, Vec3 a, Vec3 b, Vec3 c, Vec3 d, Vec3 normal)
    {
        triangles.Add(new Triangle3D(a, b, c, normal));
        triangles.Add(new Triangle3D(a, c, d, normal));
    }

    private async Task<RealSupportedGeometrySample?> TryLoadRealSupportedGeometryAsync()
    {
        var repoRoot = FindRepositoryRoot();
        var dbPath = Path.Combine(repoRoot, "debug", "data", "findamodel.db");
        if (!File.Exists(dbPath))
            return null;

        await using var connection = new SqliteConnection($"Data Source={dbPath}");
        await connection.OpenAsync();

        var modelsRoot = await ReadModelsRootAsync(connection);
        if (string.IsNullOrWhiteSpace(modelsRoot) || !Directory.Exists(modelsRoot))
            return null;

        var loader = new ModelLoaderService(NullLoggerFactory.Instance);
        foreach (var candidate in await ReadSupportedModelCandidatesAsync(connection))
        {
            var filePath = BuildModelPath(modelsRoot, candidate);
            if (!File.Exists(filePath))
                continue;

            var geometry = await loader.LoadModelAsync(filePath, candidate.FileType);
            if (geometry == null || geometry.Triangles.Count == 0)
                continue;

            return new RealSupportedGeometrySample(filePath, geometry);
        }

        return null;
    }

    private static async Task<string?> ReadModelsRootAsync(SqliteConnection connection)
    {
        await using var command = connection.CreateCommand();
        command.CommandText = "SELECT ModelsDirectoryPath FROM AppConfigs WHERE Id = 1 LIMIT 1";
        var value = await command.ExecuteScalarAsync();
        return value as string;
    }

    private static async Task<List<RealModelCandidate>> ReadSupportedModelCandidatesAsync(SqliteConnection connection)
    {
        await using var command = connection.CreateCommand();
        command.CommandText = """
            SELECT FileName, Directory, FileType
            FROM Models
            WHERE CalculatedSupported = 1
              AND lower(FileType) IN ('stl', 'obj')
            ORDER BY COALESCE(PreviewGeneratedAt, GeometryCalculatedAt, CachedAt) DESC, FileSize ASC
            LIMIT 50
            """;

        var candidates = new List<RealModelCandidate>();
        await using var reader = await command.ExecuteReaderAsync();
        while (await reader.ReadAsync())
        {
            candidates.Add(new RealModelCandidate(
                reader.GetString(0),
                reader.IsDBNull(1) ? string.Empty : reader.GetString(1),
                reader.GetString(2)));
        }

        return candidates;
    }

    private static string BuildModelPath(string modelsRoot, RealModelCandidate model)
    {
        var relativeDirectory = (model.Directory ?? string.Empty)
            .Replace('/', Path.DirectorySeparatorChar)
            .TrimStart(Path.DirectorySeparatorChar);
        return string.IsNullOrWhiteSpace(relativeDirectory)
            ? Path.Combine(modelsRoot, model.FileName)
            : Path.Combine(modelsRoot, relativeDirectory, model.FileName);
    }

    private static string FindRepositoryRoot()
    {
        var current = AppContext.BaseDirectory;
        while (!string.IsNullOrWhiteSpace(current))
        {
            if (File.Exists(Path.Combine(current, "findamodel.sln")))
                return current;

            current = Directory.GetParent(current)?.FullName ?? string.Empty;
        }

        throw new InvalidOperationException("Could not locate repository root.");
    }

    private sealed record RealSupportedGeometrySample(string FilePath, LoadedGeometry Geometry);
    private sealed record RealModelCandidate(string FileName, string Directory, string FileType);
}