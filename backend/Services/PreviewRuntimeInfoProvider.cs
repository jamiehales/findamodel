namespace findamodel.Services;

public interface IPreviewRuntimeInfoProvider
{
    bool GpuEnabled { get; }
    bool GpuAvailable { get; }
    string RendererName { get; }
}

public sealed class PreviewRuntimeInfoProvider(
    IConfiguration configuration,
    GlPreviewContext glPreviewContext) : IPreviewRuntimeInfoProvider
{
    public bool GpuEnabled => configuration.GetValue("Preview:UseGpu", defaultValue: true);

    public bool GpuAvailable => glPreviewContext.IsAvailable;

    public string RendererName => string.IsNullOrWhiteSpace(glPreviewContext.RendererName)
        ? "unavailable"
        : glPreviewContext.RendererName;
}