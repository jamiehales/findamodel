namespace findamodel.Models;

public record IndexingFilePlan(string RelativePath, string FileType);

public record IndexingFileResult(
    string RelativePath,
    string FileType,
    string Status,
    bool IsNew,
    bool WasUpdated,
    bool GeneratedPreview,
    bool GeneratedHull,
    bool GeneratedAiTags,
    bool GeneratedAiDescription,
    string? AiGenerationReason,
    string? Message,
    double DurationMs);

public interface IIndexingProgressReporter
{
    Task OnScanStartedAsync(int totalFiles);
    Task OnFilesDiscoveredAsync(IReadOnlyList<IndexingFilePlan> files);
    Task OnFileProcessedAsync(IndexingFileResult fileResult);
    Task OnLogAsync(string level, string message, string? relativePath = null);
}
