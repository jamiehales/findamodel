namespace findamodel.Services;

internal static class LocalLlmGenerationDefaults
{
    public const float Temperature = 0.2f;
    public const float TopP = 0.9f;
    public const int TopK = 50;
    public const float RepeatPenalty = 1.05f;

    public const int MinOutputTokens = 64;
    public const int InternalMaxOutputTokens = 2048;

    public static int ResolveOllamaOutputTokenLimit(int requestedMaxOutputTokens) =>
        Math.Max(MinOutputTokens, requestedMaxOutputTokens);

    public static int ResolveInternalOutputTokenLimit(int requestedMaxOutputTokens) =>
        Math.Clamp(requestedMaxOutputTokens, MinOutputTokens, InternalMaxOutputTokens);
}
