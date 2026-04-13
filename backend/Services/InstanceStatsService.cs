using System.Reflection;
using System.Runtime.InteropServices;
using findamodel.Data;
using findamodel.Models;
using Microsoft.EntityFrameworkCore;

namespace findamodel.Services;

public class InstanceStatsService(
    IDbContextFactory<ModelCacheContext> dbFactory,
    IPreviewRuntimeInfoProvider previewRuntimeInfoProvider,
    IConfiguration configuration,
    IHostEnvironment hostEnvironment)
{
    public async Task<InstanceStatsDto> GetAsync()
    {
        await using var db = await dbFactory.CreateDbContextAsync();

        var modelCount = await db.Models.AsNoTracking().CountAsync();
        var modelsWithPreviews = await db.Models.AsNoTracking().CountAsync(m => m.PreviewImagePath != null);
        var modelsWithGeneratedTags = await db.Models.AsNoTracking().CountAsync(m => m.GeneratedTagsJson != null);
        var modelsWithGeneratedDescriptions = await db.Models.AsNoTracking().CountAsync(m => m.GeneratedDescription != null);
        var directoryConfigCount = await db.DirectoryConfigs.AsNoTracking().CountAsync();
        var printingListCount = await db.PrintingLists.AsNoTracking().CountAsync();
        var metadataDictionaryValueCount = await db.MetadataDictionaryValues.AsNoTracking().CountAsync();

        var internalLlmGpuEnabled = configuration.GetValue("LocalLlm:Internal:UseGpu", defaultValue: true);
        var internalLlmGpuLayerCount = Math.Max(0, configuration.GetValue("LocalLlm:Internal:GpuLayerCount", 35));

        return new InstanceStatsDto(
            ApplicationVersion: ResolveApplicationVersion(),
            Environment: hostEnvironment.EnvironmentName,
            FrameworkVersion: RuntimeInformation.FrameworkDescription,
            OperatingSystem: RuntimeInformation.OSDescription,
            PreviewGenerationVersion: ModelPreviewService.CurrentPreviewGenerationVersion,
            HullGenerationVersion: HullCalculationService.CurrentHullGenerationVersion,
            PreviewGpuEnabled: previewRuntimeInfoProvider.GpuEnabled,
            PreviewGpuAvailable: previewRuntimeInfoProvider.GpuAvailable,
            InternalLlmGpuEnabled: internalLlmGpuEnabled,
            InternalLlmGpuLayerCount: internalLlmGpuEnabled ? internalLlmGpuLayerCount : 0,
            ModelCount: modelCount,
            ModelsWithPreviews: modelsWithPreviews,
            ModelsWithGeneratedTags: modelsWithGeneratedTags,
            ModelsWithGeneratedDescriptions: modelsWithGeneratedDescriptions,
            DirectoryConfigCount: directoryConfigCount,
            PrintingListCount: printingListCount,
            MetadataDictionaryValueCount: metadataDictionaryValueCount);
    }

    private static string ResolveApplicationVersion()
    {
        var assembly = typeof(InstanceStatsService).Assembly;
        var informationalVersion = assembly
            .GetCustomAttribute<AssemblyInformationalVersionAttribute>()?
            .InformationalVersion;

        if (!string.IsNullOrWhiteSpace(informationalVersion))
            return informationalVersion;

        return assembly.GetName().Version?.ToString() ?? "unknown";
    }
}