namespace findamodel.Services;

public static class ModelsDirectoryPathResolver
{
    public static string Resolve(string configuredPath, bool requireExisting = false, Func<string, bool>? directoryExists = null)
    {
        if (string.IsNullOrWhiteSpace(configuredPath))
            throw new ArgumentException("Models directory path is required.", nameof(configuredPath));

        directoryExists ??= Directory.Exists;
        var trimmedPath = configuredPath.Trim();

        if (OperatingSystem.IsMacOS() && TryResolveMacSmbPath(trimmedPath, requireExisting, directoryExists, out var resolvedSmbPath))
            return resolvedSmbPath;

        var fullPath = Path.GetFullPath(trimmedPath);
        if (requireExisting && !directoryExists(fullPath))
            throw new ArgumentException($"Models directory does not exist: {fullPath}", nameof(configuredPath));

        return fullPath;
    }

    private static bool TryResolveMacSmbPath(string path, bool requireExisting, Func<string, bool> directoryExists, out string resolved)
    {
        resolved = string.Empty;

        if (!Uri.TryCreate(path, UriKind.Absolute, out var uri)
            || !string.Equals(uri.Scheme, "smb", StringComparison.OrdinalIgnoreCase))
            return false;

        var candidates = GetMacSmbCandidates(uri).ToArray();
        if (candidates.Length == 0)
            return false;

        if (!requireExisting)
        {
            resolved = candidates[0];
            return true;
        }

        foreach (var candidate in candidates)
        {
            if (!directoryExists(candidate))
                continue;

            resolved = candidate;
            return true;
        }

        throw new ArgumentException(
            $"Models directory does not exist: {path}. On macOS, SMB URLs must point to a mounted share under /Volumes.",
            nameof(path));
    }

    private static IEnumerable<string> GetMacSmbCandidates(Uri smbUri)
    {
        var host = Uri.UnescapeDataString(smbUri.Host);
        var segments = smbUri.AbsolutePath
            .Split('/', StringSplitOptions.RemoveEmptyEntries)
            .Select(Uri.UnescapeDataString)
            .ToArray();

        if (segments.Length == 0)
        {
            if (!string.IsNullOrWhiteSpace(host))
                yield return Path.Combine("/Volumes", host);
            yield break;
        }

        var shareName = segments[0];
        var remainder = segments.Skip(1).ToArray();
        yield return Path.Combine(["/Volumes", shareName, .. remainder]);

        if (!string.IsNullOrWhiteSpace(host))
            yield return Path.Combine(["/Volumes", host, .. segments]);
    }
}