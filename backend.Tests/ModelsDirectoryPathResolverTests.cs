using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

public class ModelsDirectoryPathResolverTests
{
    [Fact]
    public void Resolve_ReturnsAbsoluteLocalPath_WhenPathIsNotSmb()
    {
        var tempPath = Path.Combine(Path.GetTempPath(), "findamodel-models");

        var resolved = ModelsDirectoryPathResolver.Resolve(tempPath);

        Assert.Equal(Path.GetFullPath(tempPath), resolved);
    }

    [Fact]
    public void Resolve_MapsSmbHostOnlyUrl_ToVolumesHostMount_OnMac()
    {
        if (!OperatingSystem.IsMacOS())
            return;

        const string expected = "/Volumes/behemoth.local";
        var resolved = ModelsDirectoryPathResolver.Resolve(
            "smb://behemoth.local",
            requireExisting: true,
            directoryExists: path => string.Equals(path, expected, StringComparison.Ordinal));

        Assert.Equal(expected, resolved);
    }

    [Fact]
    public void Resolve_MapsSmbShareUrl_ToVolumesShareMount_OnMac()
    {
        if (!OperatingSystem.IsMacOS())
            return;

        const string expected = "/Volumes/share-name/folder/models";
        var resolved = ModelsDirectoryPathResolver.Resolve(
            "smb://behemoth.local/share-name/folder/models",
            requireExisting: true,
            directoryExists: path => string.Equals(path, expected, StringComparison.Ordinal));

        Assert.Equal(expected, resolved);
    }

    [Fact]
    public void Resolve_DoesNotRequireMountedSmbPath_WhenExistenceNotRequired_OnMac()
    {
        if (!OperatingSystem.IsMacOS())
            return;

        var resolved = ModelsDirectoryPathResolver.Resolve(
            "smb://behemoth.local/share-name/folder/models",
            requireExisting: false,
            directoryExists: _ => false);

        Assert.Equal("/Volumes/share-name/folder/models", resolved);
    }

    [Fact]
    public void Resolve_ThrowsForUnmappedSmbUrl_OnMac()
    {
        if (!OperatingSystem.IsMacOS())
            return;

        var ex = Assert.Throws<ArgumentException>(() => ModelsDirectoryPathResolver.Resolve(
            "smb://behemoth.local/share-name",
            requireExisting: true,
            directoryExists: _ => false));

        Assert.Contains("SMB URLs", ex.Message, StringComparison.Ordinal);
        Assert.Equal("path", ex.ParamName);
    }
}