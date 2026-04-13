using System.IO.Compression;
using System.Text.RegularExpressions;
using System.Xml.Linq;
using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

/// <summary>
/// Validates that <see cref="ModelSaverService.Save3mf"/> produces output that conforms to the
/// 3MF Core Specification v1.3 (https://github.com/3MFConsortium/spec_core).
///
/// Test categories:
///   Package   - correct ZIP entries and OPC content-types / relationships
///   Model XML - correct namespace, required elements and attributes
///   Objects   - positive unique IDs, each object has mesh XOR components
///   Mesh      - vertex/triangle counts, in-bounds indices, no degenerate triangles
///   Build     - items reference valid objects, no duplicate objectids
///   Components- component objectids are valid, transform has 12 float values
///   Instancing- multiple placements share one base mesh; each gets its own build item
/// </summary>
public class Save3mfTests
{
    private static readonly ModelSaverService Saver = new();

    // 3MF Core Specification namespace
    private static readonly XNamespace Ns3mf =
        "http://schemas.microsoft.com/3dmanufacturing/core/2015/02";

    private static readonly XNamespace NsContentTypes =
        "http://schemas.openxmlformats.org/package/2006/content-types";

    private static readonly XNamespace NsRelationships =
        "http://schemas.openxmlformats.org/package/2006/relationships";

    // ── Helpers ─────────────────────────────────────────────────────────────────

    /// <summary>Four triangles forming a small tetrahedron - enough geometry for all tests.</summary>
    private static List<Triangle3D> UnitTetrahedron() =>
    [
        new(new Vec3(0, 0, 0),  new Vec3(10, 0, 0), new Vec3(5, 0, 10),  Vec3.Up),
        new(new Vec3(0, 0, 0),  new Vec3(5, 0, 10), new Vec3(5, 10, 5),  Vec3.Up),
        new(new Vec3(10, 0, 0), new Vec3(5, 10, 5), new Vec3(5, 0, 10),  Vec3.Up),
        new(new Vec3(0, 0, 0),  new Vec3(5, 10, 5), new Vec3(10, 0, 0),  Vec3.Up),
    ];

    /// <summary>
    /// Generates a 3MF with one base mesh object (id=1) and the supplied placements.
    /// Each placement is (objectId, transformString).  Pass nothing for a single identity placement.
    /// </summary>
    private static byte[] Simple3mf(params (int ObjectId, string Transform)[] items)
    {
        if (items.Length == 0)
            items = [(1, "1 0 0 0 1 0 0 0 1 0 0 0")];

        var objects = new[] { (1, (IReadOnlyList<Triangle3D>)UnitTetrahedron()) };
        return Saver.Save3mf(objects, items);
    }

    private static Dictionary<string, byte[]> ZipEntries(byte[] bytes)
    {
        var result = new Dictionary<string, byte[]>(StringComparer.OrdinalIgnoreCase);
        using var ms = new MemoryStream(bytes);
        using var zip = new ZipArchive(ms, ZipArchiveMode.Read);
        foreach (var entry in zip.Entries)
        {
            using var buf = new MemoryStream();
            entry.Open().CopyTo(buf);
            result[entry.FullName] = buf.ToArray();
        }
        return result;
    }

    private static XDocument ModelXml(byte[] bytes)
    {
        var entries = ZipEntries(bytes);
        Assert.True(entries.ContainsKey("3D/3dmodel.model"),
            "3D/3dmodel.model not found in package");
        return XDocument.Parse(System.Text.Encoding.UTF8.GetString(entries["3D/3dmodel.model"]));
    }

    // ── Package files ────────────────────────────────────────────────────────────

    [Fact]
    public void Package_ContainsContentTypesXml()
        => Assert.True(ZipEntries(Simple3mf()).ContainsKey("[Content_Types].xml"),
            "[Content_Types].xml is required by OPC");

    [Fact]
    public void Package_ContainsRels()
        => Assert.True(ZipEntries(Simple3mf()).ContainsKey("_rels/.rels"),
            "_rels/.rels is required by OPC");

    [Fact]
    public void Package_ContainsModelFile()
        => Assert.True(ZipEntries(Simple3mf()).ContainsKey("3D/3dmodel.model"),
            "3D model part is missing from package");

    // ── Content types ─────────────────────────────────────────────────────────────

    [Fact]
    public void ContentTypes_ModelExtensionIsCorrect()
    {
        var doc = XDocument.Parse(System.Text.Encoding.UTF8.GetString(
            ZipEntries(Simple3mf())["[Content_Types].xml"]));
        var el = doc.Root!.Elements(NsContentTypes + "Default")
            .FirstOrDefault(e => e.Attribute("Extension")?.Value == "model");

        Assert.NotNull(el);
        // 3MF Core Spec §5.1 mandates this specific content type string
        Assert.Equal("application/vnd.ms-package.3dmanufacturing-3dmodel+xml", el.Attribute("ContentType")?.Value);
    }

    [Fact]
    public void ContentTypes_RelsExtensionIsCorrect()
    {
        var doc = XDocument.Parse(System.Text.Encoding.UTF8.GetString(
            ZipEntries(Simple3mf())["[Content_Types].xml"]));
        var el = doc.Root!.Elements(NsContentTypes + "Default")
            .FirstOrDefault(e => e.Attribute("Extension")?.Value == "rels");

        Assert.NotNull(el);
        Assert.Equal(
            "application/vnd.openxmlformats-package.relationships+xml",
            el.Attribute("ContentType")?.Value);
    }

    // ── Relationships ─────────────────────────────────────────────────────────────

    [Fact]
    public void Rels_Has3mfRelationshipWithCorrectTypeAndTarget()
    {
        var doc = XDocument.Parse(System.Text.Encoding.UTF8.GetString(
            ZipEntries(Simple3mf())["_rels/.rels"]));

        var rel = doc.Root!.Elements(NsRelationships + "Relationship")
            .FirstOrDefault(e =>
                e.Attribute("Type")?.Value ==
                "http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel");

        Assert.NotNull(rel);

        // Target must resolve to the 3D model file (with or without leading /)
        var target = rel.Attribute("Target")?.Value?.TrimStart('/');
        Assert.Equal("3D/3dmodel.model", target, ignoreCase: true);
    }

    // ── Model XML namespace / structure ─────────────────────────────────────────

    [Fact]
    public void Model_RootElementHas3mfNamespace()
    {
        var doc = ModelXml(Simple3mf());
        Assert.Equal(
            "http://schemas.microsoft.com/3dmanufacturing/core/2015/02",
            doc.Root!.Name.NamespaceName);
    }

    [Fact]
    public void Model_HasUnitAttribute_WithValidValue()
    {
        var doc = ModelXml(Simple3mf());
        var unit = doc.Root!.Attribute("unit")?.Value;

        Assert.NotNull(unit);
        Assert.Contains(unit,
            new[] { "micron", "millimeter", "centimeter", "inch", "foot", "meter" });
    }

    [Fact]
    public void Model_HasResourcesElement()
        => Assert.NotNull(ModelXml(Simple3mf()).Root!.Element(Ns3mf + "resources"));

    [Fact]
    public void Model_HasBuildElement()
        => Assert.NotNull(ModelXml(Simple3mf()).Root!.Element(Ns3mf + "build"));

    // ── Object IDs ──────────────────────────────────────────────────────────────

    [Fact]
    public void Objects_IdsArePositiveIntegers()
    {
        foreach (var obj in ModelXml(Simple3mf()).Descendants(Ns3mf + "object"))
        {
            var idStr = obj.Attribute("id")?.Value;
            Assert.True(int.TryParse(idStr, out var id),
                $"Object id '{idStr}' is not an integer");
            Assert.True(id >= 1, $"Object id {id} must be ≥ 1 (spec §4.1 requires positive integer)");
        }
    }

    [Fact]
    public void Objects_IdsAreUnique()
    {
        var ids = ModelXml(Simple3mf())
            .Descendants(Ns3mf + "object")
            .Select(o => o.Attribute("id")!.Value)
            .ToList();

        Assert.Equal(ids.Count, ids.Distinct().Count());
    }

    // ── Object structure ─────────────────────────────────────────────────────────

    [Fact]
    public void Objects_HaveMeshXorComponents_NotBothOrNeither()
    {
        // 3MF spec §4.1: an object MUST contain either a mesh or a components child element.
        // Having both or neither is a conformance error.
        foreach (var obj in ModelXml(Simple3mf()).Descendants(Ns3mf + "object"))
        {
            var hasMesh = obj.Element(Ns3mf + "mesh") != null;
            var hasComponents = obj.Element(Ns3mf + "components") != null;

            Assert.True(hasMesh ^ hasComponents,
                $"Object id={obj.Attribute("id")?.Value} must have mesh XOR components " +
                $"(hasMesh={hasMesh}, hasComponents={hasComponents})");
        }
    }

    // ── Mesh validity ─────────────────────────────────────────────────────────────

    [Fact]
    public void Mesh_HasAtLeastThreeVertices()
    {
        foreach (var obj in ModelXml(Simple3mf()).Descendants(Ns3mf + "object")
                                                 .Where(o => o.Element(Ns3mf + "mesh") != null))
        {
            var count = obj.Element(Ns3mf + "mesh")!
                           .Element(Ns3mf + "vertices")!
                           .Elements(Ns3mf + "vertex").Count();

            Assert.True(count >= 3, $"Mesh has {count} vertices; 3MF spec requires at least 3");
        }
    }

    [Fact]
    public void Mesh_HasAtLeastOneTriangle()
    {
        foreach (var obj in ModelXml(Simple3mf()).Descendants(Ns3mf + "object")
                                                 .Where(o => o.Element(Ns3mf + "mesh") != null))
        {
            var count = obj.Element(Ns3mf + "mesh")!
                           .Element(Ns3mf + "triangles")!
                           .Elements(Ns3mf + "triangle").Count();

            Assert.True(count >= 1, $"Mesh has {count} triangles; 3MF spec requires at least 1");
        }
    }

    [Fact]
    public void Mesh_VerticesAreDeduplicated()
    {
        // The implementation shares vertices, so the vertex count must be strictly less than
        // 3 × triangle count for any mesh whose triangles share corners (e.g. the tetrahedron).
        // The tetrahedron has 4 unique corners and 4 triangles → vCount (4) < tCount × 3 (12).
        foreach (var obj in ModelXml(Simple3mf()).Descendants(Ns3mf + "object")
                                                 .Where(o => o.Element(Ns3mf + "mesh") != null))
        {
            var mesh = obj.Element(Ns3mf + "mesh")!;
            var vCount = mesh.Element(Ns3mf + "vertices")!.Elements(Ns3mf + "vertex").Count();
            var tCount = mesh.Element(Ns3mf + "triangles")!.Elements(Ns3mf + "triangle").Count();

            Assert.True(vCount >= 3, $"Vertex count {vCount} must be ≥ 3");
            Assert.True(vCount <= tCount * 3, $"Vertex count {vCount} must be ≤ tCount×3 = {tCount * 3}");
            // The tetrahedron specifically has 4 unique corners.
            Assert.Equal(4, vCount);
        }
    }

    [Fact]
    public void Triangles_IndicesAreInBounds()
    {
        foreach (var obj in ModelXml(Simple3mf()).Descendants(Ns3mf + "object")
                                                 .Where(o => o.Element(Ns3mf + "mesh") != null))
        {
            var mesh = obj.Element(Ns3mf + "mesh")!;
            var vertexCount = mesh.Element(Ns3mf + "vertices")!
                                  .Elements(Ns3mf + "vertex").Count();

            foreach (var tri in mesh.Element(Ns3mf + "triangles")!.Elements(Ns3mf + "triangle"))
            {
                foreach (var attr in new[] { "v1", "v2", "v3" })
                {
                    var i = int.Parse(tri.Attribute(attr)!.Value);
                    Assert.True(i >= 0 && i < vertexCount,
                        $"Triangle attribute {attr}={i} is out of bounds [0, {vertexCount})");
                }
            }
        }
    }

    [Fact]
    public void Triangles_VertexIndicesAreDistinct()
    {
        // 3MF spec §4.1.4: v1, v2, v3 MUST be distinct (degenerate triangles are forbidden).
        foreach (var tri in ModelXml(Simple3mf()).Descendants(Ns3mf + "triangle"))
        {
            var v1 = int.Parse(tri.Attribute("v1")!.Value);
            var v2 = int.Parse(tri.Attribute("v2")!.Value);
            var v3 = int.Parse(tri.Attribute("v3")!.Value);

            Assert.True(v1 != v2 && v1 != v3 && v2 != v3,
                $"Degenerate triangle (duplicate index): v1={v1}, v2={v2}, v3={v3}");
        }
    }

    // ── Build items ───────────────────────────────────────────────────────────────

    [Fact]
    public void Build_HasAtLeastOneItem()
    {
        var count = ModelXml(Simple3mf())
            .Root!.Element(Ns3mf + "build")!
            .Elements(Ns3mf + "item").Count();

        Assert.True(count >= 1, "Build element must contain at least one item");
    }

    [Fact]
    public void BuildItems_ReferenceExistingObjectIds()
    {
        var doc = ModelXml(Simple3mf());
        var objectIds = doc.Descendants(Ns3mf + "object")
                           .Select(o => o.Attribute("id")!.Value)
                           .ToHashSet();

        foreach (var item in doc.Root!.Element(Ns3mf + "build")!.Elements(Ns3mf + "item"))
        {
            var refId = item.Attribute("objectid")?.Value;
            Assert.NotNull(refId);
            Assert.Contains(refId, objectIds);
        }
    }

    [Fact]
    public void BuildItems_ObjectIdsAreUnique()
    {
        // 3MF spec §3.4: the same objectid MUST NOT appear more than once in <build>.
        var ids = ModelXml(Simple3mf())
            .Root!.Element(Ns3mf + "build")!
            .Elements(Ns3mf + "item")
            .Select(i => i.Attribute("objectid")!.Value)
            .ToList();

        Assert.Equal(ids.Count, ids.Distinct().Count());
    }

    [Fact]
    public void BuildItems_DoNotDirectlyReferenceMeshObjects()
    {
        // The instancing pattern wraps each placement in a component object.
        // <build> items must reference those wrapper objects, not the raw mesh objects.
        var doc = ModelXml(Simple3mf());
        var meshIds = doc.Descendants(Ns3mf + "object")
                         .Where(o => o.Element(Ns3mf + "mesh") != null)
                         .Select(o => o.Attribute("id")!.Value)
                         .ToHashSet();

        foreach (var item in doc.Root!.Element(Ns3mf + "build")!.Elements(Ns3mf + "item"))
            Assert.DoesNotContain(item.Attribute("objectid")!.Value, meshIds);
    }

    // ── Components ────────────────────────────────────────────────────────────────

    [Fact]
    public void Components_ReferenceExistingObjectIds()
    {
        var doc = ModelXml(Simple3mf());
        var objectIds = doc.Descendants(Ns3mf + "object")
                           .Select(o => o.Attribute("id")!.Value)
                           .ToHashSet();

        foreach (var c in doc.Descendants(Ns3mf + "component"))
        {
            var refId = c.Attribute("objectid")?.Value;
            Assert.NotNull(refId);
            Assert.Contains(refId, objectIds);
        }
    }

    [Fact]
    public void Components_TransformHasTwelveSpaceSeparatedFloats()
    {
        // 3MF spec §3.3: transform is 12 real numbers in row-major order.
        foreach (var c in ModelXml(Simple3mf()).Descendants(Ns3mf + "component"))
        {
            var transform = c.Attribute("transform")?.Value;
            Assert.NotNull(transform);

            var parts = transform.Split(' ', StringSplitOptions.RemoveEmptyEntries);
            Assert.Equal(12, parts.Length);

            foreach (var p in parts)
                Assert.True(
                    double.TryParse(p, System.Globalization.NumberStyles.Float,
                        System.Globalization.CultureInfo.InvariantCulture, out _),
                    $"Transform value '{p}' could not be parsed as a float");
        }
    }

    // ── Instancing ────────────────────────────────────────────────────────────────

    [Fact]
    public void Instancing_ThreePlacementsShareOneMeshObject()
    {
        // Three placements of the same model → only one base mesh object in <resources>.
        var bytes = Simple3mf(
            (1, "1 0 0 0 1 0 0 0 1 0 0 0"),
            (1, "1 0 0 0 1 0 0 0 1 50 0 0"),
            (1, "1 0 0 0 1 0 0 0 1 100 0 0"));

        var meshObjects = ModelXml(bytes)
            .Descendants(Ns3mf + "object")
            .Where(o => o.Element(Ns3mf + "mesh") != null)
            .ToList();

        Assert.Single(meshObjects);
    }

    [Fact]
    public void Instancing_ThreePlacementsProduceThreeBuildItems()
    {
        var bytes = Simple3mf(
            (1, "1 0 0 0 1 0 0 0 1 0 0 0"),
            (1, "1 0 0 0 1 0 0 0 1 50 0 0"),
            (1, "1 0 0 0 1 0 0 0 1 100 0 0"));

        var count = ModelXml(bytes)
            .Root!.Element(Ns3mf + "build")!
            .Elements(Ns3mf + "item").Count();

        Assert.Equal(3, count);
    }

    [Fact]
    public void Instancing_ThreePlacementsHaveUniqueWrapperObjectIds()
    {
        var bytes = Simple3mf(
            (1, "1 0 0 0 1 0 0 0 1 0 0 0"),
            (1, "1 0 0 0 1 0 0 0 1 50 0 0"),
            (1, "1 0 0 0 1 0 0 0 1 100 0 0"));

        // All wrapper objects referenced from <build> must have distinct IDs
        var doc = ModelXml(bytes);
        var buildIds = doc.Root!.Element(Ns3mf + "build")!
            .Elements(Ns3mf + "item")
            .Select(i => i.Attribute("objectid")!.Value)
            .ToList();

        Assert.Equal(buildIds.Count, buildIds.Distinct().Count());
    }

    // ── lib3mf round-trip validation ─────────────────────────────────────────────
    //
    // These tests load the generated 3MF bytes into the precompiled lib3mf library
    // (v2.5.0, lib3mf/lib3mf.dll) to verify conformance beyond what XML parsing
    // alone can guarantee.  lib3mf is the reference implementation used by major
    // slicers (PrusaSlicer, Bambu Studio, etc.) and its reader enforces the full
    // 3MF Core Specification.

    /// <summary>Loads the generated 3MF via lib3mf and returns the parsed model.</summary>
    private static Lib3MF.CModel LoadViaLib3mf(byte[] bytes)
    {
        var model = Lib3MF.Wrapper.CreateModel();
        var reader = model.QueryReader("3mf");
        reader.ReadFromBuffer(bytes);
        return model;
    }

    /// <summary>Collects all build items from the iterator into a list.</summary>
    private static List<Lib3MF.CBuildItem> AllBuildItems(Lib3MF.CModel model)
    {
        var iter = model.GetBuildItems();
        var list = new List<Lib3MF.CBuildItem>();
        while (iter.MoveNext())
            list.Add(iter.GetCurrent());
        return list;
    }

    /// <summary>Collects all mesh objects from the iterator into a list.</summary>
    private static List<Lib3MF.CMeshObject> AllMeshObjects(Lib3MF.CModel model)
    {
        var iter = model.GetMeshObjects();
        var list = new List<Lib3MF.CMeshObject>();
        while (iter.MoveNext())
            list.Add(iter.GetCurrentMeshObject());
        return list;
    }

    /// <summary>Collects all components-objects from the iterator into a list.</summary>
    private static List<Lib3MF.CComponentsObject> AllComponentsObjects(Lib3MF.CModel model)
    {
        var iter = model.GetComponentsObjects();
        var list = new List<Lib3MF.CComponentsObject>();
        while (iter.MoveNext())
            list.Add(iter.GetCurrentComponentsObject());
        return list;
    }

    [Fact]
    public void Lib3mf_Reads_WithoutWarnings()
    {
        // lib3mf raises warnings for non-fatal spec violations.
        // A well-formed 3MF should produce zero warnings.
        var reader = Lib3MF.Wrapper.CreateModel().QueryReader("3mf");
        reader.ReadFromBuffer(Simple3mf());
        Assert.Equal(0u, reader.GetWarningCount());
    }

    // ── lib3mf instancing tests ───────────────────────────────────────────────────

    [Fact]
    public void Lib3mf_Instancing_ThreePlacements_OneMeshObjectInModel()
    {
        var bytes = Simple3mf(
            (1, "1 0 0 0 1 0 0 0 1 0 0 0"),
            (1, "1 0 0 0 1 0 0 0 1 50 0 0"),
            (1, "1 0 0 0 1 0 0 0 1 100 0 0"));

        var model = LoadViaLib3mf(bytes);
        Assert.Single(AllMeshObjects(model));
    }

    [Fact]
    public void Lib3mf_Instancing_ThreePlacements_ThreeBuildItemsInModel()
    {
        var bytes = Simple3mf(
            (1, "1 0 0 0 1 0 0 0 1 0 0 0"),
            (1, "1 0 0 0 1 0 0 0 1 50 0 0"),
            (1, "1 0 0 0 1 0 0 0 1 100 0 0"));

        var model = LoadViaLib3mf(bytes);
        Assert.Equal(3, AllBuildItems(model).Count);
    }

    [Fact]
    public void Lib3mf_Instancing_ThreePlacements_BuildItemsReferenceComponentsObjects()
    {
        // Each build item must point to a ComponentsObject, not a raw MeshObject.
        var bytes = Simple3mf(
            (1, "1 0 0 0 1 0 0 0 1 0 0 0"),
            (1, "1 0 0 0 1 0 0 0 1 50 0 0"),
            (1, "1 0 0 0 1 0 0 0 1 100 0 0"));

        var model = LoadViaLib3mf(bytes);
        foreach (var item in AllBuildItems(model))
            Assert.True(item.GetObjectResource().IsComponentsObject(),
                "Build item must reference a ComponentsObject (not a mesh object directly)");
    }

    [Fact]
    public void Lib3mf_Instancing_ThreePlacements_AllComponentsReferenceTheSameMesh()
    {
        // All three wrapper objects must point their single component at the one shared mesh.
        var bytes = Simple3mf(
            (1, "1 0 0 0 1 0 0 0 1 0 0 0"),
            (1, "1 0 0 0 1 0 0 0 1 50 0 0"),
            (1, "1 0 0 0 1 0 0 0 1 100 0 0"));

        var model = LoadViaLib3mf(bytes);
        var meshId = AllMeshObjects(model).Single().GetModelResourceID();
        var compObjs = AllComponentsObjects(model);

        Assert.Equal(3, compObjs.Count);
        foreach (var co in compObjs)
        {
            Assert.Equal(1u, co.GetComponentCount());
            var referencedId = co.GetComponent(0).GetObjectResourceID();
            Assert.Equal(meshId, referencedId);
        }
    }

    [Fact]
    public void Lib3mf_Instancing_ThreePlacements_TranslationsAreDistinct()
    {
        // Each placement uses a different X translation; verify lib3mf reads them back correctly.
        var bytes = Simple3mf(
            (1, "1 0 0 0 1 0 0 0 1 0 0 0"),
            (1, "1 0 0 0 1 0 0 0 1 50 0 0"),
            (1, "1 0 0 0 1 0 0 0 1 100 0 0"));

        var model = LoadViaLib3mf(bytes);
        var compObjs = AllComponentsObjects(model);

        // Collect the X-translation from each component's transform.
        // sTransform.Fields[3][0] is the translation-X in the lib3mf 4×3 column-major layout.
        var translationsX = compObjs
            .Select(co => co.GetComponent(0).GetTransform().Fields[3][0])
            .OrderBy(x => x)
            .ToArray();

        Assert.Equal(new[] { 0f, 50f, 100f }, translationsX);
    }

    // ── lib3mf round-trip comparison: our output vs lib3mf-generated ──────────────

    /// <summary>Unit cube from (0,0,0) to (1,1,1): 6 faces × 2 triangles = 12 triangles.</summary>
    private static List<Triangle3D> UnitCube() =>
    [
        // Bottom (y=0, normal -Y)
        new(new Vec3(0,0,0), new Vec3(1,0,1), new Vec3(1,0,0), new Vec3(0,-1,0)),
        new(new Vec3(0,0,0), new Vec3(0,0,1), new Vec3(1,0,1), new Vec3(0,-1,0)),
        // Top (y=1, normal +Y)
        new(new Vec3(0,1,0), new Vec3(1,1,0), new Vec3(1,1,1), new Vec3(0,1,0)),
        new(new Vec3(0,1,0), new Vec3(1,1,1), new Vec3(0,1,1), new Vec3(0,1,0)),
        // Front (z=1, normal +Z)
        new(new Vec3(0,0,1), new Vec3(1,1,1), new Vec3(1,0,1), new Vec3(0,0,1)),
        new(new Vec3(0,0,1), new Vec3(0,1,1), new Vec3(1,1,1), new Vec3(0,0,1)),
        // Back (z=0, normal -Z)
        new(new Vec3(0,0,0), new Vec3(1,0,0), new Vec3(1,1,0), new Vec3(0,0,-1)),
        new(new Vec3(0,0,0), new Vec3(1,1,0), new Vec3(0,1,0), new Vec3(0,0,-1)),
        // Left (x=0, normal -X)
        new(new Vec3(0,0,0), new Vec3(0,1,0), new Vec3(0,1,1), new Vec3(-1,0,0)),
        new(new Vec3(0,0,0), new Vec3(0,1,1), new Vec3(0,0,1), new Vec3(-1,0,0)),
        // Right (x=1, normal +X)
        new(new Vec3(1,0,0), new Vec3(1,1,1), new Vec3(1,1,0), new Vec3(1,0,0)),
        new(new Vec3(1,0,0), new Vec3(1,0,1), new Vec3(1,1,1), new Vec3(1,0,0)),
    ];

    /// <summary>
    /// Generates a UV sphere approximation centred at the origin with radius 1.
    /// Default: 6 stacks × 8 slices → 80 triangles, clearly distinct from the 12-triangle cube.
    /// Normals are set to Vec3.Up (not used in 3MF output; only meaningful for STL/preview).
    /// </summary>
    private static List<Triangle3D> UnitSphere(int stacks = 6, int slices = 8)
    {
        var tris = new List<Triangle3D>();
        static Vec3 S(float phi, float theta) =>
            new(MathF.Sin(phi) * MathF.Cos(theta), MathF.Cos(phi), MathF.Sin(phi) * MathF.Sin(theta));

        for (int i = 0; i < stacks; i++)
        {
            float phi0 = MathF.PI * i / stacks;
            float phi1 = MathF.PI * (i + 1) / stacks;
            for (int j = 0; j < slices; j++)
            {
                float th0 = 2 * MathF.PI * j / slices;
                float th1 = 2 * MathF.PI * (j + 1) / slices;
                var v00 = S(phi0, th0); var v01 = S(phi0, th1);
                var v10 = S(phi1, th0); var v11 = S(phi1, th1);
                if (i == 0) tris.Add(new(v00, v11, v10, Vec3.Up)); // top cap
                else if (i == stacks - 1) tris.Add(new(v00, v01, v10, Vec3.Up)); // bottom cap
                else { tris.Add(new(v00, v01, v11, Vec3.Up)); tris.Add(new(v00, v11, v10, Vec3.Up)); }
            }
        }
        return tris;
    }

    /// <summary>
    /// Builds a lib3mf sTransform representing a pure translation (identity rotation).
    /// Fields layout: Fields[col][row], 4 columns × 3 rows, column-major.
    /// Fields[3] = translation.
    /// </summary>
    private static Lib3MF.sTransform MakeTranslationTransform(float x, float y, float z) =>
        new()
        {
            Fields =
            [
                [1f, 0f, 0f],
                [0f, 1f, 0f],
                [0f, 0f, 1f],
                [x,  y,  z ],
            ]
        };

    /// <summary>
    /// Builds a 3MF package from scratch using lib3mf's write API.
    /// Each object produces one shared-vertex mesh. Each placement produces one
    /// ComponentsObject (referencing the appropriate mesh) and one BuildItem.
    /// </summary>
    private static byte[] BuildViaLib3mf(
        IReadOnlyList<(int Id, IReadOnlyList<Triangle3D> Triangles)> objects,
        IReadOnlyList<(int ObjectId, float X, float Y, float Z)> placements)
    {
        var model = Lib3MF.Wrapper.CreateModel();
        var meshById = new Dictionary<int, Lib3MF.CMeshObject>();

        foreach (var (id, triangles) in objects)
        {
            var meshObj = model.AddMeshObject();
            var vertIdx = new Dictionary<(float, float, float), uint>();
            uint GetOrAdd(Vec3 v)
            {
                var key = (v.X, v.Y, v.Z);
                if (!vertIdx.TryGetValue(key, out var idx))
                {
                    idx = meshObj.AddVertex(new Lib3MF.sPosition { Coordinates = [v.X, v.Y, v.Z] });
                    vertIdx[key] = idx;
                }
                return idx;
            }
            foreach (var tri in triangles)
                meshObj.AddTriangle(new Lib3MF.sTriangle
                {
                    Indices = [GetOrAdd(tri.V0), GetOrAdd(tri.V1), GetOrAdd(tri.V2)]
                });
            meshById[id] = meshObj;
        }

        var identity = MakeTranslationTransform(0, 0, 0);
        foreach (var (objectId, x, y, z) in placements)
        {
            var compObj = model.AddComponentsObject();
            compObj.AddComponent(meshById[objectId], MakeTranslationTransform(x, y, z));
            model.AddBuildItem(compObj, identity);
        }

        var writer = model.QueryWriter("3mf");
        writer.WriteToBuffer(out var bytes);
        return bytes;
    }

    [Fact]
    public void Lib3mf_RoundTrip_TwoMeshesTwentyInstancesEach_ComparesWithLib3mfGenerated()
    {
        // Object 1 (cube):   20 positions lerped from (-10, 10, 10) to ( 10, 10, 10)
        // Object 2 (sphere): 20 positions lerped from ( 10,-10,-10) to (-10, 10, 10)
        const int count = 20;
        var cubePositions = Enumerable.Range(0, count)
            .Select(i => { float t = (float)i / (count - 1); return (-10f + t * 20f, 10f, 10f); })
            .ToArray();
        var spherePositions = Enumerable.Range(0, count)
            .Select(i => { float t = (float)i / (count - 1); return (10f - t * 20f, -10f + t * 20f, -10f + t * 20f); })
            .ToArray();

        var cubeTriangles = UnitCube();
        var sphereTriangles = UnitSphere(); // 80 triangles - distinct from cube's 12

        // ── Our library ───────────────────────────────────────────────────────────
        // 3MF transform: "m00 m01 m02 m10 m11 m12 m20 m21 m22 tx ty tz" (identity rotation)
        // G6 matches lib3mf's 6-significant-figure output for translation values.
        static string Tfm((float, float, float) p) =>
            FormattableString.Invariant($"1 0 0 0 1 0 0 0 1 {p.Item1:G6} {p.Item2:G6} {p.Item3:G6}");

        var items = cubePositions.Select(p => (1, Tfm(p)))
            .Concat(spherePositions.Select(p => (2, Tfm(p))))
            .ToArray();
        var ourBytes = Saver.Save3mf(
            [(1, (IReadOnlyList<Triangle3D>)cubeTriangles), (2, (IReadOnlyList<Triangle3D>)sphereTriangles)],
            items);

        // ── lib3mf reference ──────────────────────────────────────────────────────
        var refBytes = BuildViaLib3mf(
            [(1, cubeTriangles), (2, sphereTriangles)],
            [
                .. cubePositions.Select(p   => (1, p.Item1, p.Item2, p.Item3)),
                .. spherePositions.Select(p => (2, p.Item1, p.Item2, p.Item3)),
            ]);

        var ourModel = LoadViaLib3mf(ourBytes);
        var refModel = LoadViaLib3mf(refBytes);
        var diffs = new List<string>();

        // ── Binary & formatted-XML equivalence ───────────────────────────────────
        // Production UUIDs are random, so raw bytes can never be identical between two
        // independent runs.  We therefore compare the *UUID-normalised* formatted XML of
        // each ZIP entry, which strips that non-determinism and lets us assert that
        // everything else is structurally equivalent to lib3mf's output.
        var binaryIssues = new List<string>();
        bool rawEqual = ourBytes.SequenceEqual(refBytes);
        if (rawEqual)
            binaryIssues.Add("Raw bytes are identical (unexpected - UUIDs should differ)");

        static string NormalizeXml(string xml)
        {
            // Strip random production UUIDs so they never cause mismatches.
            xml = Regex.Replace(xml,
                @"p:UUID=""[0-9a-fA-F\-]+""",
                "p:UUID=\"00000000-0000-0000-0000-000000000000\"");

            // Round vertex coordinate values to 4 decimal places so that 1-ULP float32
            // rounding differences between .NET MathF and lib3mf's libm don't cause mismatches.
            try
            {
                var doc = XDocument.Parse(xml);
                XNamespace ns = "http://schemas.microsoft.com/3dmanufacturing/core/2015/02";
                foreach (var vertex in doc.Descendants(ns + "vertex"))
                {
                    foreach (var attrName in new[] { "x", "y", "z" })
                    {
                        var attr = vertex.Attribute(attrName);
                        if (attr != null &&
                            float.TryParse(attr.Value,
                                System.Globalization.NumberStyles.Float,
                                System.Globalization.CultureInfo.InvariantCulture, out var v))
                        {
                            attr.SetValue(MathF.Round(v, 4).ToString(
                                "G6", System.Globalization.CultureInfo.InvariantCulture));
                        }
                    }
                }
                return doc.ToString(SaveOptions.None);
            }
            catch
            {
                return xml;
            }
        }

        var ourZip = ZipEntries(ourBytes);
        var refZip = ZipEntries(refBytes);
        bool allXmlMatch = true;

        foreach (var key in ourZip.Keys.Union(refZip.Keys, StringComparer.OrdinalIgnoreCase).OrderBy(k => k))
        {
            bool inO = ourZip.TryGetValue(key, out var ob);
            bool inR = refZip.TryGetValue(key, out var rb);
            if (!inO) { binaryIssues.Add($"Entry missing from ours: {key}"); allXmlMatch = false; continue; }
            if (!inR) { binaryIssues.Add($"Entry missing from ref:  {key}"); allXmlMatch = false; continue; }

            bool isText = key.EndsWith(".model", StringComparison.OrdinalIgnoreCase)
                       || key.EndsWith(".xml", StringComparison.OrdinalIgnoreCase)
                       || key.EndsWith(".rels", StringComparison.OrdinalIgnoreCase);
            if (!isText) continue;

            var ourXml = NormalizeXml(FormatXml(ob!));
            var refXml = NormalizeXml(FormatXml(rb!));
            if (ourXml == refXml) continue;

            allXmlMatch = false;
            var tmp = Path.GetTempPath();
            var safe = key.Replace('/', '_').Replace('\\', '_');
            var ourPath = Path.Combine(tmp, $"our_{safe}");
            var refPath = Path.Combine(tmp, $"ref_{safe}");
            File.WriteAllText(ourPath, ourXml, System.Text.Encoding.UTF8);
            File.WriteAllText(refPath, refXml, System.Text.Encoding.UTF8);
            binaryIssues.Add($"Entry '{key}' formatted XML differs (UUID-normalised):");
            binaryIssues.Add($"  ours: {ourPath}");
            binaryIssues.Add($"  ref:  {refPath}");
        }

        Assert.True(allXmlMatch,
            $"Formatted-XML differences ({binaryIssues.Count} issue(s)):\n" +
            string.Join("\n", binaryIssues.Select((d, i) => $"  [{i + 1}] {d}")));

        // ── Total build item count ────────────────────────────────────────────────
        int expectedTotal = count * 2;
        var ourItems = AllBuildItems(ourModel);
        var refItems = AllBuildItems(refModel);
        if (ourItems.Count != expectedTotal)
            diffs.Add($"Build item count: ours={ourItems.Count}, expected={expectedTotal}");
        if (refItems.Count != expectedTotal)
            diffs.Add($"Build item count: ref={refItems.Count}, expected={expectedTotal}");

        // ── Total mesh object count ───────────────────────────────────────────────
        var ourMeshes = AllMeshObjects(ourModel);
        var refMeshes = AllMeshObjects(refModel);
        if (ourMeshes.Count != 2) diffs.Add($"Mesh object count: ours={ourMeshes.Count}, expected=2");
        if (refMeshes.Count != 2) diffs.Add($"Mesh object count: ref={refMeshes.Count}, expected=2");

        // ── Per-mesh geometry + translations ──────────────────────────────────────
        // Meshes are identified by their triangle count: cube=12, sphere=80.
        void CheckMesh(
            string label,
            Lib3MF.CModel model,
            string name,
            List<Triangle3D> expectedGeom,
            (float, float, float)[] expectedPos)
        {
            int expTri = expectedGeom.Count;
            int expVert = UniqueVertexCount(expectedGeom);
            const float eps = 1e-3f;

            // Find the mesh whose triangle count matches.
            Lib3MF.CMeshObject? mesh = null;
            foreach (var m in AllMeshObjects(model))
            {
                m.GetTriangleIndices(out var t);
                if (t.Length == expTri) { mesh = m; break; }
            }
            if (mesh == null) { diffs.Add($"[{label}] {name}: no mesh with {expTri} triangles"); return; }

            mesh.GetTriangleIndices(out var tris);
            mesh.GetVertices(out var verts);
            if (tris.Length != expTri) diffs.Add($"[{label}] {name} triangle count: got={tris.Length}, expected={expTri}");
            if (verts.Length != expVert) diffs.Add($"[{label}] {name} vertex count: got={verts.Length}, expected={expVert}");

            // Collect the component objects that reference this mesh.
            uint meshId = mesh.GetModelResourceID();
            var compObjs = AllComponentsObjects(model)
                .Where(co => co.GetComponent(0).GetObjectResourceID() == meshId)
                .ToList();
            if (compObjs.Count != count)
            {
                diffs.Add($"[{label}] {name} instance count: got={compObjs.Count}, expected={count}");
                return;
            }

            // Validate all three translation axes against sorted expected values.
            float[] Act(int axis) => compObjs
                .Select(co => co.GetComponent(0).GetTransform().Fields[3][axis])
                .OrderBy(v => v).ToArray();
            float[] Exp(int axis) => [.. expectedPos
                .Select(p => axis == 0 ? p.Item1 : axis == 1 ? p.Item2 : p.Item3)
                .OrderBy(v => v)];

            string[] axisNames = ["X", "Y", "Z"];
            for (int axis = 0; axis < 3; axis++)
            {
                var act = Act(axis); var exp = Exp(axis);
                for (int i = 0; i < count; i++)
                    if (MathF.Abs(act[i] - exp[i]) > eps)
                        diffs.Add($"[{label}] {name} {axisNames[axis]}[{i}]: got={act[i]:G9}, expected={exp[i]:G9}");
            }
        }

        if (ourMeshes.Count == 2)
        {
            CheckMesh("ours", ourModel, "cube", cubeTriangles, cubePositions);
            CheckMesh("ours", ourModel, "sphere", sphereTriangles, spherePositions);
        }
        if (refMeshes.Count == 2)
        {
            CheckMesh("ref", refModel, "cube", cubeTriangles, cubePositions);
            CheckMesh("ref", refModel, "sphere", sphereTriangles, spherePositions);
        }

        Assert.True(diffs.Count == 0,
            $"Differences ({diffs.Count} issue(s)):\n" +
            string.Join("\n", diffs.Select((d, i) => $"  [{i + 1}] {d}")));
    }

    // Returns the number of unique vertex positions in a triangle list (shared-vertex count).
    private static int UniqueVertexCount(List<Triangle3D> tris) =>
        tris.SelectMany(t => new[] { (t.V0.X, t.V0.Y, t.V0.Z), (t.V1.X, t.V1.Y, t.V1.Z), (t.V2.X, t.V2.Y, t.V2.Z) })
            .Distinct()
            .Count();

    private static string FormatXml(byte[] xmlBytes)
    {
        try
        {
            var doc = XDocument.Parse(System.Text.Encoding.UTF8.GetString(xmlBytes));
            return doc.ToString(SaveOptions.None);
        }
        catch
        {
            return System.Text.Encoding.UTF8.GetString(xmlBytes);
        }
    }
}
