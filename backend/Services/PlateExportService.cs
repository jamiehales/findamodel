using System.Globalization;
using System.Numerics;
using findamodel.Models;

namespace findamodel.Services;

public interface IPlateGenerationProgressReporter
{
    void StartStage(int totalEntries, string? entryName = null);
    void MarkCurrentEntry(string entryName);
    void MarkEntryCompleted();
}

public sealed class PlateExportUnprocessableException(string message) : Exception(message);

public sealed record PlateExportFileResult(
    byte[] Content,
    string ContentType,
    string FileName,
    string? Warning,
    IReadOnlyList<string> SkippedModels);

public sealed class PlateExportService(
    ModelService modelService,
    ModelLoaderService loaderService,
    ModelSaverService saverService,
    PlateSliceRasterService sliceRasterService,
    PrinterService printerService,
    IConfiguration config)
{
    private const int MaxConcurrentModelLoads = 2;

    private static readonly HashSet<string> NonGeometryTypes =
        new(StringComparer.OrdinalIgnoreCase) { "lys", "lyt", "ctb" };

    private readonly record struct ExportPlacement(Guid ModelId, double XMm, double YMm, double AngleRad);

    public static string NormalizeFormat(string? format)
    {
        var normalized = (format ?? "3mf").ToLowerInvariant();
        if (normalized is not ("3mf" or "stl" or "glb" or "pngzip" or "pngzip_mesh" or "pngzip_orthographic"))
            throw new ArgumentException($"Unsupported format '{format}'. Supported: 3mf, stl, glb, pngzip, pngzip_mesh, pngzip_orthographic");

        return normalized;
    }

    public static string GetFileName(string format) => format switch
    {
        "stl" => "plate.stl",
        "glb" => "plate.glb",
        "pngzip_mesh" => "plate-slices-mesh.zip",
        "pngzip_orthographic" => "plate-slices-orthographic.zip",
        "pngzip" => "plate-slices.zip",
        _ => "plate.3mf",
    };

    private static PngSliceExportMethod ResolvePngSliceMethod(string format) => format switch
    {
        "pngzip_orthographic" => PngSliceExportMethod.OrthographicProjection,
        "pngzip_mesh" => PngSliceExportMethod.MeshIntersection,
        _ => PlateSliceRasterService.DefaultZipDownloadMethod,
    };

    public async Task<PlateExportFileResult> GeneratePlateAsync(
        GeneratePlateRequest request,
        IPlateGenerationProgressReporter? progressReporter = null,
        CancellationToken cancellationToken = default)
    {
        var modelsPath = config["Models:DirectoryPath"];
        if (string.IsNullOrWhiteSpace(modelsPath))
            throw new InvalidOperationException("Models:DirectoryPath not configured");

        var format = NormalizeFormat(request.Format);

        var requestedModelIds = new HashSet<Guid>();
        var requestPlacements = new List<(PlacementDto Placement, Guid ModelId)>(request.Placements.Count);
        foreach (var placement in request.Placements)
        {
            cancellationToken.ThrowIfCancellationRequested();

            if (!Guid.TryParse(placement.ModelId, out var modelId))
                throw new ArgumentException($"Invalid model ID: {placement.ModelId}");

            requestedModelIds.Add(modelId);
            requestPlacements.Add((placement, modelId));
        }

        var modelInfoById = await modelService.GetModelFileInfoByIdsAsync(requestedModelIds);
        if (modelInfoById.Count != requestedModelIds.Count)
        {
            var missingModelId = requestedModelIds.First(id => !modelInfoById.ContainsKey(id));
            throw new KeyNotFoundException($"Model not found: {missingModelId}");
        }

        var exportPlacements = new List<ExportPlacement>(requestPlacements.Count);
        var skippedModelNames = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        var uniqueGeometryModelIds = new List<Guid>(requestedModelIds.Count);
        var seenGeometryModelIds = new HashSet<Guid>();

        foreach (var (placement, modelId) in requestPlacements)
        {
            cancellationToken.ThrowIfCancellationRequested();
            var modelInfo = modelInfoById[modelId];

            if (NonGeometryTypes.Contains(modelInfo.FileType))
            {
                skippedModelNames.Add(modelInfo.FileName);
                progressReporter?.MarkEntryCompleted();
                continue;
            }

            exportPlacements.Add(new ExportPlacement(modelId, placement.XMm, placement.YMm, placement.AngleRad));

            if (!seenGeometryModelIds.Add(modelId))
            {
                progressReporter?.MarkEntryCompleted();
                continue;
            }

            uniqueGeometryModelIds.Add(modelId);
        }

        var objectIdByModelId = uniqueGeometryModelIds
            .Select((modelId, index) => (modelId, objectId: index + 1))
            .ToDictionary(x => x.modelId, x => x.objectId);

        var maxConcurrentLoads = Math.Max(1, Math.Min(MaxConcurrentModelLoads, uniqueGeometryModelIds.Count));
        using var loadGate = new SemaphoreSlim(maxConcurrentLoads, maxConcurrentLoads);

        var geometryResults = await Task.WhenAll(uniqueGeometryModelIds.Select(async modelId =>
        {
            await loadGate.WaitAsync(cancellationToken);
            try
            {
                var modelInfo = modelInfoById[modelId];
                progressReporter?.MarkCurrentEntry(modelInfo.FileName);

                var fullPath = string.IsNullOrEmpty(modelInfo.Directory)
                    ? Path.Combine(modelsPath, modelInfo.FileName)
                    : Path.Combine(modelsPath, modelInfo.Directory, modelInfo.FileName);

                if (!File.Exists(fullPath))
                    throw new FileNotFoundException($"Model file not found on disk: {modelInfo.FileName}", fullPath);

                var geometry = await loaderService.LoadModelAsync(fullPath, modelInfo.FileType);
                if (geometry == null)
                    throw new InvalidOperationException($"Failed to parse geometry for: {modelInfo.FileName}");

                progressReporter?.MarkEntryCompleted();
                return (ModelId: modelId, Geometry: geometry);
            }
            finally
            {
                loadGate.Release();
            }
        }));

        var geometryByModelId = geometryResults.ToDictionary(x => x.ModelId, x => x.Geometry);

        if (exportPlacements.Count == 0)
            throw new PlateExportUnprocessableException("No geometry-based models were included in the export request.");

        var warning = skippedModelNames.Count > 0
            ? "Some models were skipped because they do not contain exportable geometry (LYS/LYT/CTB)."
            : null;

        var result = format switch
        {
            "3mf" => new PlateExportFileResult(
                Generate3mf(exportPlacements, geometryByModelId, objectIdByModelId),
                "application/vnd.ms-3mf",
                GetFileName(format),
                warning,
                skippedModelNames.OrderBy(name => name, StringComparer.OrdinalIgnoreCase).ToList()),
            "stl" => new PlateExportFileResult(
                GenerateStl(exportPlacements, geometryByModelId),
                "model/stl",
                GetFileName(format),
                warning,
                skippedModelNames.OrderBy(name => name, StringComparer.OrdinalIgnoreCase).ToList()),
            "glb" => new PlateExportFileResult(
                GenerateGlb(exportPlacements, geometryByModelId, objectIdByModelId),
                "model/gltf-binary",
                GetFileName(format),
                warning,
                skippedModelNames.OrderBy(name => name, StringComparer.OrdinalIgnoreCase).ToList()),
            "pngzip" or "pngzip_mesh" or "pngzip_orthographic" => await GeneratePngZipAsync(
                request,
                format,
                exportPlacements,
                geometryByModelId,
                warning,
                skippedModelNames.OrderBy(name => name, StringComparer.OrdinalIgnoreCase).ToList(),
                progressReporter,
                cancellationToken),
            _ => throw new NotImplementedException($"Unhandled format '{format}'"),
        };

        return result;
    }

    private byte[] Generate3mf(
        IReadOnlyList<ExportPlacement> placements,
        Dictionary<Guid, LoadedGeometry> geometryByModelId,
        Dictionary<Guid, int> objectIdByModelId)
    {
        string Compute3mfTransform(double angleRad, double xMm, double yMm)
        {
            float cosA = MathF.Cos((float)angleRad);
            float sinA = MathF.Sin((float)angleRad);
            return string.Create(CultureInfo.InvariantCulture,
                $"{cosA:G9} {-sinA:G9} 0 {sinA:G9} {cosA:G9} 0 0 0 1 {-xMm:G9} {yMm:G9} 0");
        }

        var objects = new List<(int Id, IReadOnlyList<Triangle3D> Triangles)>(geometryByModelId.Count);
        foreach (var (modelId, geometry) in geometryByModelId)
        {
            var sourceTriangles = geometry.Triangles;
            var zUpTriangles = new Triangle3D[sourceTriangles.Count];
            for (int i = 0; i < sourceTriangles.Count; i++)
            {
                var t = sourceTriangles[i];
                zUpTriangles[i] = new Triangle3D(
                    YUpToZUp(t.V0),
                    YUpToZUp(t.V1),
                    YUpToZUp(t.V2),
                    YUpToZUp(t.Normal));
            }

            objects.Add((objectIdByModelId[modelId], zUpTriangles));
        }

        var items = new List<(int ObjectId, string Transform)>(placements.Count);
        foreach (var placement in placements)
            items.Add((objectIdByModelId[placement.ModelId], Compute3mfTransform(placement.AngleRad, placement.XMm, placement.YMm)));

        return saverService.Save3mf(objects, items);
    }

    private byte[] GenerateStl(IReadOnlyList<ExportPlacement> placements, Dictionary<Guid, LoadedGeometry> geometryByModelId)
    {
        var placedTriangles = GeneratePlacedTriangles(placements, geometryByModelId, convertToZUp: true);
        return saverService.SaveStl(placedTriangles.Count, placedTriangles, "findamodel plate");
    }

    private async Task<PlateExportFileResult> GeneratePngZipAsync(
        GeneratePlateRequest request,
        string format,
        IReadOnlyList<ExportPlacement> placements,
        Dictionary<Guid, LoadedGeometry> geometryByModelId,
        string? warning,
        IReadOnlyList<string> skippedModels,
        IPlateGenerationProgressReporter? progressReporter,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();

        var printer = request.PrinterConfigId.HasValue
            ? await printerService.GetByIdAsync(request.PrinterConfigId.Value)
            : await printerService.GetDefaultAsync();

        if (printer == null)
            throw new InvalidOperationException("A printer configuration is required for PNG slice export.");

        var normalizedPlacements = placements
            .Select(p => new ExportPlacement(
                p.ModelId,
                p.XMm - (printer.BedWidthMm / 2f),
                (printer.BedDepthMm / 2f) - p.YMm,
                p.AngleRad))
            .ToList();

        var placedTriangles = GeneratePlacedTriangles(normalizedPlacements, geometryByModelId, convertToZUp: false);
        var content = sliceRasterService.GenerateSliceArchive(
            placedTriangles,
            printer.BedWidthMm,
            printer.BedDepthMm,
            printer.PixelWidth,
            printer.PixelHeight,
            ResolvePngSliceMethod(format),
            progressReporter: progressReporter,
            cancellationToken: cancellationToken);

        return new PlateExportFileResult(
            content,
            "application/zip",
            GetFileName(format),
            warning,
            skippedModels);
    }

    private List<Triangle3D> GeneratePlacedTriangles(
        IReadOnlyList<ExportPlacement> placements,
        Dictionary<Guid, LoadedGeometry> geometryByModelId,
        bool convertToZUp)
    {
        Vec3 PlaceVertex(Vec3 v, float sinA, float cosA, float xMm, float yMm)
            => new(v.X * cosA - v.Z * sinA + xMm, v.Y, v.X * sinA + v.Z * cosA + yMm);

        Vec3 RotateY(Vec3 n, float sinA, float cosA)
            => new(n.X * cosA - n.Z * sinA, n.Y, n.X * sinA + n.Z * cosA);

        var merged = new List<Triangle3D>();
        foreach (var placement in placements)
        {
            var geometry = geometryByModelId[placement.ModelId];
            float sinA = MathF.Sin((float)placement.AngleRad);
            float cosA = MathF.Cos((float)placement.AngleRad);
            foreach (var tri in geometry.Triangles)
            {
                var v0 = PlaceVertex(tri.V0, sinA, cosA, (float)placement.XMm, (float)placement.YMm);
                var v1 = PlaceVertex(tri.V1, sinA, cosA, (float)placement.XMm, (float)placement.YMm);
                var v2 = PlaceVertex(tri.V2, sinA, cosA, (float)placement.XMm, (float)placement.YMm);
                var normal = RotateY(tri.Normal, sinA, cosA);

                if (convertToZUp)
                {
                    merged.Add(new Triangle3D(
                        YUpToZUp(v0),
                        YUpToZUp(v1),
                        YUpToZUp(v2),
                        YUpToZUp(normal)));
                }
                else
                {
                    merged.Add(new Triangle3D(v0, v1, v2, normal));
                }
            }
        }

        return merged;
    }

    private byte[] GenerateGlb(
        IReadOnlyList<ExportPlacement> placements,
        Dictionary<Guid, LoadedGeometry> geometryByModelId,
        Dictionary<Guid, int> objectIdByModelId)
    {
        Matrix4x4 ComputeGlbTransform(double angleRad, double xMm, double yMm)
        {
            var rotation = Matrix4x4.CreateRotationY((float)angleRad);
            var translation = Matrix4x4.CreateTranslation((float)xMm, 0f, (float)yMm);
            return rotation * translation;
        }

        var glbObjects = geometryByModelId
            .Select(kvp => (objectIdByModelId[kvp.Key], (IReadOnlyList<Triangle3D>)kvp.Value.Triangles))
            .ToList();

        var glbItems = placements
            .Select(p => (objectIdByModelId[p.ModelId], ComputeGlbTransform(p.AngleRad, p.XMm, p.YMm)))
            .ToList();

        return saverService.SaveGlb(glbObjects, glbItems);
    }

    private static Vec3 YUpToZUp(Vec3 v) => new(-v.X, v.Z, v.Y);
}
