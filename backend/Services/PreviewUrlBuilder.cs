namespace findamodel.Services;

internal static class PreviewUrlBuilder
{
    public static string Build(Guid modelId, int? previewGenerationVersion) =>
        $"/api/models/{modelId}/preview?v={previewGenerationVersion ?? 0}&includeSupports=true";
}