using System.Text.Json;
using findamodel.Models;
using findamodel.Services;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace findamodel.Tests;

public class DirectoryConfigReaderModelMetadataTests
{
    private static DirectoryConfigReader MakeReader() =>
        new(NullLoggerFactory.Instance);

    // ── model_metadata absent ─────────────────────────────────────────────────

    [Fact]
    public async Task ParseConfigFileAsync_NoModelMetadataSection_ReturnsNullJson()
    {
        var yaml = "creator: Alice\n";
        var path = Path.GetTempFileName();
        try
        {
            await File.WriteAllTextAsync(path, yaml);
            var result = await MakeReader().ParseConfigFileAsync(path);
            Assert.Null(result?.ModelMetadataJson);
        }
        finally { File.Delete(path); }
    }

    // ── model_metadata present ────────────────────────────────────────────────

    [Fact]
    public async Task ParseConfigFileAsync_ModelMetadataWithName_SerializesCorrectly()
    {
        var yaml = """
            creator: Alice
            model_metadata:
              dragon.stl:
                name: "Fire Dragon"
            """;
        var path = Path.GetTempFileName();
        try
        {
            await File.WriteAllTextAsync(path, yaml);
            var result = await MakeReader().ParseConfigFileAsync(path);

            Assert.NotNull(result?.ModelMetadataJson);
            var dict = JsonSerializer.Deserialize<Dictionary<string, ModelMetadataEntry>>(
                result!.ModelMetadataJson!,
                new JsonSerializerOptions { PropertyNameCaseInsensitive = true });

            Assert.NotNull(dict);
            Assert.True(dict!.ContainsKey("dragon.stl"));
            Assert.Equal("Fire Dragon", dict["dragon.stl"].Name);
        }
        finally { File.Delete(path); }
    }

    [Fact]
    public async Task ParseConfigFileAsync_ModelMetadataWithPartName_ParsedCorrectly()
    {
        var yaml = """
            model_metadata:
              bust_torso.stl:
                name: "Knight Bust"
                part_name: "Torso"
            """;
        var path = Path.GetTempFileName();
        try
        {
            await File.WriteAllTextAsync(path, yaml);
            var result = await MakeReader().ParseConfigFileAsync(path);

            Assert.NotNull(result?.ModelMetadataJson);
            var dict = JsonSerializer.Deserialize<Dictionary<string, ModelMetadataEntry>>(
                result!.ModelMetadataJson!,
                new JsonSerializerOptions { PropertyNameCaseInsensitive = true });

            var entry = dict!["bust_torso.stl"];
            Assert.Equal("Knight Bust", entry.Name);
            Assert.Equal("Torso", entry.PartName);
        }
        finally { File.Delete(path); }
    }

    [Fact]
    public async Task ParseConfigFileAsync_ModelMetadataAllNullFields_EntryOmitted()
    {
        // If all fields are missing/null an entry should not be added
        var yaml = """
            model_metadata:
              dragon.stl: {}
            """;
        var path = Path.GetTempFileName();
        try
        {
            await File.WriteAllTextAsync(path, yaml);
            var result = await MakeReader().ParseConfigFileAsync(path);
            // No non-null fields → dict will be empty → null returned
            Assert.Null(result?.ModelMetadataJson);
        }
        finally { File.Delete(path); }
    }
}
