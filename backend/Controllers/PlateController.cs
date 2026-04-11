using System.Globalization;
using System.Numerics;
using Microsoft.AspNetCore.Mvc;
using findamodel.Services;

namespace findamodel.Controllers;

[ApiController]
[Route("api/[controller]")]
public class PlateController(
    ModelService modelService,
    ModelLoaderService loaderService,
    ModelSaverService saverService,
    IConfiguration config) : ControllerBase
{
    private static readonly HashSet<string> NonGeometryTypes =
        new(StringComparer.OrdinalIgnoreCase) { "lys", "lyt", "ctb" };

    /// <summary>
    /// Generates a build-plate file from the supplied model placements.
    ///
    /// Supported formats (request.Format field, default "3mf"):
    ///   3mf — 3MF package with instancing; each unique model stored once as a mesh resource.
    ///   stl — merged binary STL with all placements baked into a single triangle soup.
    ///
    /// Each placement specifies a model ID, its XY position on the plate (in mm), and a
    /// rotation angle (radians, counter-clockwise when viewed from above).
    /// </summary>
    [HttpPost("generate")]
    public async Task<IActionResult> GeneratePlate([FromBody] GeneratePlateRequest request)
    {
        var modelsPath = config["Models:DirectoryPath"];
        if (string.IsNullOrEmpty(modelsPath))
            return StatusCode(500, "Models:DirectoryPath not configured");

        var format = (request.Format ?? "3mf").ToLowerInvariant();
        if (format is not ("3mf" or "stl" or "glb"))
            return BadRequest($"Unsupported format '{request.Format}'. Supported: 3mf, stl, glb");

        // Load each unique model once.
        var exportPlacements = new List<PlacementDto>(request.Placements.Count);
        var skippedModelNames = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        var geometryByModelId = new Dictionary<Guid, LoadedGeometry>();
        var objectIdByModelId = new Dictionary<Guid, int>();
        int nextObjectId = 1;

        foreach (var placement in request.Placements)
        {
            if (!Guid.TryParse(placement.ModelId, out var modelId))
                return BadRequest($"Invalid model ID: {placement.ModelId}");

            if (geometryByModelId.ContainsKey(modelId))
            {
                exportPlacements.Add(placement);
                continue;
            }

            var model = await modelService.GetModelAsync(modelId);
            if (model == null)
                return NotFound($"Model not found: {placement.ModelId}");

            if (NonGeometryTypes.Contains(model.FileType))
            {
                skippedModelNames.Add(model.FileName);
                continue;
            }

            var fullPath = string.IsNullOrEmpty(model.Directory)
                ? Path.Combine(modelsPath, model.FileName)
                : Path.Combine(modelsPath, model.Directory, model.FileName);

            if (!System.IO.File.Exists(fullPath))
                return NotFound($"Model file not found on disk: {model.FileName}");

            var geometry = await loaderService.LoadModelAsync(fullPath, model.FileType);
            if (geometry == null)
                return StatusCode(500, $"Failed to parse geometry for: {model.FileName}");

            geometryByModelId[modelId] = geometry;
            objectIdByModelId[modelId] = nextObjectId++;
            exportPlacements.Add(placement);
        }

        if (skippedModelNames.Count > 0)
        {
            Response.Headers.Append("X-Plate-Skipped-Models", string.Join(',', skippedModelNames));
            Response.Headers.Append(
                "X-Plate-Warning",
                "Some models were skipped because they do not contain exportable geometry (LYS/LYT/CTB).");
        }

        if (exportPlacements.Count == 0)
            return UnprocessableEntity("No geometry-based models were included in the export request.");

        switch (format)
        {
            case "3mf":
                return Generate3mf(exportPlacements, geometryByModelId, objectIdByModelId);
            case "stl":
                return GenerateStl(exportPlacements, geometryByModelId);
            case "glb":
                return GenerateGlb(exportPlacements, geometryByModelId, objectIdByModelId);
            default:
                throw new NotImplementedException($"Unhandled format '{format}'");
        }
    }

    private IActionResult Generate3mf(
        IReadOnlyList<PlacementDto> placements,
        Dictionary<Guid, LoadedGeometry> geometryByModelId,
        Dictionary<Guid, int> objectIdByModelId)
    {
        /// <summary>
        /// Computes the 3MF transform string for a placement, expressed in Z-up space.
        ///
        /// The base mesh is stored after applying YUpToZUp: (x,y,z) → (−x,z,y).
        /// Composing that with RotateY(A) + Translate(XMm,YMm) + YUpToZUp gives:
        ///   rotation around Z by −A  +  translation (−XMm, YMm, 0).
        ///
        /// 3MF matrix layout "m00 m01 m02 m10 m11 m12 m20 m21 m22 m30 m31 m32" where
        ///   p'x = m00·px + m10·py + m20·pz + m30, etc.
        /// </summary>
        string Compute3mfTransform(double angleRad, double xMm, double yMm)
        {
            float cosA = MathF.Cos((float)angleRad);
            float sinA = MathF.Sin((float)angleRad);
            return string.Create(CultureInfo.InvariantCulture,
                $"{cosA:G9} {-sinA:G9} 0 {sinA:G9} {cosA:G9} 0 0 0 1 {-xMm:G9} {yMm:G9} 0");
        }

        // 3MF: base geometry in Z-up, placement baked into per-item transforms (instancing).
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

        return File(saverService.Save3mf(objects, items), "application/vnd.ms-3mf", "plate.3mf");
    }

    private IActionResult GenerateStl(IReadOnlyList<PlacementDto> placements, Dictionary<Guid, LoadedGeometry> geometryByModelId)
    {
        /// <summary>Rotate around Y axis then translate in the XZ plane (used for STL output).</summary>
        Vec3 PlaceVertex(Vec3 v, float sinA, float cosA, float xMm, float yMm)
            => new(v.X * cosA - v.Z * sinA + xMm, v.Y, v.X * sinA + v.Z * cosA + yMm);

        /// <summary>Rotate around Y axis — direction only, no translation (used for STL normals).</summary>
        Vec3 RotateY(Vec3 n, float sinA, float cosA)
            => new(n.X * cosA - n.Z * sinA, n.Y, n.X * sinA + n.Z * cosA);

        // Merge all placements into a single triangle soup, converting to Z-up.
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
        return File(saverService.SaveStl(merged, "findamodel plate"), "model/stl", "plate.stl");
    }

    private IActionResult GenerateGlb(
        IReadOnlyList<PlacementDto> placements,
        Dictionary<Guid, LoadedGeometry> geometryByModelId,
        Dictionary<Guid, int> objectIdByModelId)
    {
        /// <summary>
        /// Computes the Y-up placement transform for GLB output as a System.Numerics matrix.
        ///
        /// The mesh is centred at origin in Y-up space.  The placement rotates it around the Y axis
        /// by <paramref name="angleRad"/> then translates by (XMm, 0, YMm).
        ///
        /// System.Numerics.Matrix4x4 uses row-vector convention, so CreateRotationY(A) gives
        /// exactly the same rotation as the inline PlaceVertex formula:
        ///   x' = x·cos(A) − z·sin(A),  z' = x·sin(A) + z·cos(A).
        /// </summary>
        Matrix4x4 ComputeGlbTransform(double angleRad, double xMm, double yMm)
        {
            var rotation = Matrix4x4.CreateRotationY((float)angleRad);
            var translation = Matrix4x4.CreateTranslation((float)xMm, 0f, (float)yMm);
            return rotation * translation;
        }

        // GLB: geometry stays in Y-up (glTF's native system — no axis conversion).
        // Each unique mesh is stored once; node-level instancing references it N times.
        var glbObjects = geometryByModelId
            .Select(kvp => (objectIdByModelId[kvp.Key], (IReadOnlyList<Triangle3D>)kvp.Value.Triangles))
            .ToList();

        var glbItems = placements
            .Select(p => (objectIdByModelId[Guid.Parse(p.ModelId)], ComputeGlbTransform(p.AngleRad, p.XMm, p.YMm)))
            .ToList();

        return File(saverService.SaveGlb(glbObjects, glbItems), "model/gltf-binary", "plate.glb");
    }

    /// <summary>Y-up (x,y,z) → Z-up (−x,z,y). This transform is its own inverse.</summary>
    static private Vec3 YUpToZUp(Vec3 v) => new(-v.X, v.Z, v.Y);
}

/// <param name="ModelId">GUID of the model.</param>
/// <param name="InstanceIndex">0-based index when the same model appears multiple times on the plate.</param>
/// <param name="XMm">X position on the plate in mm.</param>
/// <param name="YMm">Y position on the plate in mm (maps to Z in 3D Y-up space).</param>
/// <param name="AngleRad">Rotation around the vertical axis in radians (counter-clockwise when viewed from above).</param>
public record PlacementDto(string ModelId, int InstanceIndex, double XMm, double YMm, double AngleRad);

/// <param name="Format">Output format: "3mf" (default) or "stl".</param>
public record GeneratePlateRequest(IReadOnlyList<PlacementDto> Placements, string? Format = null);
