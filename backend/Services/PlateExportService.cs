using System.Globalization;
using System.Numerics;
using findamodel.Models;

namespace findamodel.Services;

public interface IPlateGenerationProgressReporter
{
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
    IConfiguration config)
{
    private static readonly HashSet<string> NonGeometryTypes =
        new(StringComparer.OrdinalIgnoreCase) { "lys", "lyt", "ctb" };

    public static string NormalizeFormat(string? format)
    {
        var normalized = (format ?? "3mf").ToLowerInvariant();
        if (normalized is not ("3mf" or "stl" or "glb"))
            throw new ArgumentException($"Unsupported format '{format}'. Supported: 3mf, stl, glb");

        return normalized;
    }

    public static string GetFileName(string format) => format switch
    {
        "stl" => "plate.stl",
        "glb" => "plate.glb",
        _ => "plate.3mf",
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

        var exportPlacements = new List<PlacementDto>(request.Placements.Count);
        var skippedModelNames = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        var geometryByModelId = new Dictionary<Guid, LoadedGeometry>();
        var objectIdByModelId = new Dictionary<Guid, int>();
        var modelInfoById = new Dictionary<Guid, (string FileName, string Directory, string FileType)>();
        var skippedModelIds = new HashSet<Guid>();
        var nextObjectId = 1;

        foreach (var placement in request.Placements)
        {
            cancellationToken.ThrowIfCancellationRequested();

            if (!Guid.TryParse(placement.ModelId, out var modelId))
                throw new ArgumentException($"Invalid model ID: {placement.ModelId}");

            if (!modelInfoById.TryGetValue(modelId, out var modelInfo))
            {
                var model = await modelService.GetModelAsync(modelId);
                if (model == null)
                    throw new KeyNotFoundException($"Model not found: {placement.ModelId}");

                modelInfo = (model.FileName, model.Directory, model.FileType);
                modelInfoById[modelId] = modelInfo;
            }

            progressReporter?.MarkCurrentEntry(modelInfo.FileName);

            if (skippedModelIds.Contains(modelId))
            {
                skippedModelNames.Add(modelInfo.FileName);
                progressReporter?.MarkEntryCompleted();
                continue;
            }

            if (geometryByModelId.ContainsKey(modelId))
            {
                exportPlacements.Add(placement);
                progressReporter?.MarkEntryCompleted();
                continue;
            }

            if (NonGeometryTypes.Contains(modelInfo.FileType))
            {
                skippedModelIds.Add(modelId);
                skippedModelNames.Add(modelInfo.FileName);
                progressReporter?.MarkEntryCompleted();
                continue;
            }

            var fullPath = string.IsNullOrEmpty(modelInfo.Directory)
                ? Path.Combine(modelsPath, modelInfo.FileName)
                : Path.Combine(modelsPath, modelInfo.Directory, modelInfo.FileName);

            if (!File.Exists(fullPath))
                throw new FileNotFoundException($"Model file not found on disk: {modelInfo.FileName}", fullPath);

            var geometry = await loaderService.LoadModelAsync(fullPath, modelInfo.FileType);
            if (geometry == null)
                throw new InvalidOperationException($"Failed to parse geometry for: {modelInfo.FileName}");

            geometryByModelId[modelId] = geometry;
            objectIdByModelId[modelId] = nextObjectId++;
            exportPlacements.Add(placement);
            progressReporter?.MarkEntryCompleted();
        }

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
            _ => throw new NotImplementedException($"Unhandled format '{format}'"),
        };

        return result;
    }

    private byte[] Generate3mf(
        IReadOnlyList<PlacementDto> placements,
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

        var objects = new List<(int Id, IReadOnlyList<Triangle3D> Triangles)>();
        foreach (var (modelId, geometry) in geometryByModelId)
        {
            var zUpTriangles = geometry.Triangles
                .Select(t => new Triangle3D(YUpToZUp(t.V0), YUpToZUp(t.V1), YUpToZUp(t.V2), YUpToZUp(t.Normal)))
                .ToList();
            objects.Add((objectIdByModelId[modelId], zUpTriangles));
        }

        var items = new List<(int ObjectId, string Transform)>();
        foreach (var placement in placements)
        {
            var modelId = Guid.Parse(placement.ModelId);
            items.Add((objectIdByModelId[modelId], Compute3mfTransform(placement.AngleRad, placement.XMm, placement.YMm)));
        }

        return saverService.Save3mf(objects, items);
    }

    private byte[] GenerateStl(IReadOnlyList<PlacementDto> placements, Dictionary<Guid, LoadedGeometry> geometryByModelId)
    {
        Vec3 PlaceVertex(Vec3 v, float sinA, float cosA, float xMm, float yMm)
            => new(v.X * cosA - v.Z * sinA + xMm, v.Y, v.X * sinA + v.Z * cosA + yMm);

        Vec3 RotateY(Vec3 n, float sinA, float cosA)
            => new(n.X * cosA - n.Z * sinA, n.Y, n.X * sinA + n.Z * cosA);

        var merged = new List<Triangle3D>();
        foreach (var placement in placements)
        {
            var modelId = Guid.Parse(placement.ModelId);
            var geometry = geometryByModelId[modelId];
            float sinA = MathF.Sin((float)placement.AngleRad);
            float cosA = MathF.Cos((float)placement.AngleRad);
            foreach (var tri in geometry.Triangles)
            {
                merged.Add(new Triangle3D(
                    YUpToZUp(PlaceVertex(tri.V0, sinA, cosA, (float)placement.XMm, (float)placement.YMm)),
                    YUpToZUp(PlaceVertex(tri.V1, sinA, cosA, (float)placement.XMm, (float)placement.YMm)),
                    YUpToZUp(PlaceVertex(tri.V2, sinA, cosA, (float)placement.XMm, (float)placement.YMm)),
                    YUpToZUp(RotateY(tri.Normal, sinA, cosA))));
            }
        }

        return saverService.SaveStl(merged, "findamodel plate");
    }

    private byte[] GenerateGlb(
        IReadOnlyList<PlacementDto> placements,
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
            .Select(p => (objectIdByModelId[Guid.Parse(p.ModelId)], ComputeGlbTransform(p.AngleRad, p.XMm, p.YMm)))
            .ToList();

        return saverService.SaveGlb(glbObjects, glbItems);
    }

    private static Vec3 YUpToZUp(Vec3 v) => new(-v.X, v.Z, v.Y);
}
