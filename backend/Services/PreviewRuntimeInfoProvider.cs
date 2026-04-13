namespace findamodel.Services;

public interface IPreviewRuntimeInfoProvider
{
    bool GpuEnabled { get; }
    bool GpuAvailable { get; }
}

public sealed class PreviewRuntimeInfoProvider(
    IConfiguration configuration,
    GlPreviewContext glPreviewContext) : IPreviewRuntimeInfoProvider
{
    public bool GpuEnabled => configuration.GetValue("Preview:UseGpu", defaultValue: true);

    public bool GpuAvailable => glPreviewContext.IsAvailable;
}