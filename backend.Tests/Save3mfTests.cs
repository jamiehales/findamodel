using System.IO.Compression;
using System.Xml.Linq;
using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

/// <summary>
/// Validates that <see cref="ModelSaverService.Save3mf"/> produces output that conforms to the
/// 3MF Core Specification v1.3 (https://github.com/3MFConsortium/spec_core).
///
/// Test categories:
///   Package   — correct ZIP entries and OPC content-types / relationships
///   Model XML — correct namespace, required elements and attributes
///   Objects   — positive unique IDs, each object has mesh XOR components
///   Mesh      — vertex/triangle counts, in-bounds indices, no degenerate triangles
///   Build     — items reference valid objects, no duplicate objectids
///   Components— component objectids are valid, transform has 12 float values
///   Instancing— multiple placements share one base mesh; each gets its own build item
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

    /// <summary>Four triangles forming a small tetrahedron — enough geometry for all tests.</summary>
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
        Assert.Equal("application/vnd.ms-3mf.model", el.Attribute("ContentType")?.Value);
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
            var hasMesh       = obj.Element(Ns3mf + "mesh")       != null;
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
    public void Mesh_VertexCountEqualsThreeTimesTriangleCount()
    {
        // The implementation writes 3 unshared vertices per triangle.
        foreach (var obj in ModelXml(Simple3mf()).Descendants(Ns3mf + "object")
                                                 .Where(o => o.Element(Ns3mf + "mesh") != null))
        {
            var mesh   = obj.Element(Ns3mf + "mesh")!;
            var vCount = mesh.Element(Ns3mf + "vertices")! .Elements(Ns3mf + "vertex")  .Count();
            var tCount = mesh.Element(Ns3mf + "triangles")!.Elements(Ns3mf + "triangle").Count();

            Assert.Equal(tCount * 3, vCount);
        }
    }

    [Fact]
    public void Triangles_IndicesAreInBounds()
    {
        foreach (var obj in ModelXml(Simple3mf()).Descendants(Ns3mf + "object")
                                                 .Where(o => o.Element(Ns3mf + "mesh") != null))
        {
            var mesh        = obj.Element(Ns3mf + "mesh")!;
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
}
