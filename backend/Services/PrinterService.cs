using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Models;
using Microsoft.EntityFrameworkCore;

namespace findamodel.Services;

public class PrinterService(IDbContextFactory<ModelCacheContext> dbFactory)
{
    public async Task<List<PrinterConfigDto>> GetAllAsync()
    {
        using var db = dbFactory.CreateDbContext();
        return await db.PrinterConfigs
            .OrderBy(p => p.IsBuiltIn ? 0 : 1)
            .ThenBy(p => p.Name)
            .Select(MapDtoProjection())
            .ToListAsync();
    }

    public async Task<PrinterConfigDto?> GetDefaultAsync()
    {
        using var db = dbFactory.CreateDbContext();
        var p = await db.PrinterConfigs.FirstOrDefaultAsync(p => p.IsDefault);
        return p == null ? null : MapDto(p);
    }

    public async Task<PrinterConfigDto?> GetByIdAsync(Guid id)
    {
        using var db = dbFactory.CreateDbContext();
        var p = await db.PrinterConfigs.FirstOrDefaultAsync(p => p.Id == id);
        return p == null ? null : MapDto(p);
    }

    public async Task<(PrinterConfigDto? Dto, string? Error)> CreateAsync(CreatePrinterConfigRequest request)
    {
        var name = request.Name.Trim();
        if (string.IsNullOrWhiteSpace(name))
            return (null, "Name is required.");
        if (request.BedWidthMm <= 0 || request.BedDepthMm <= 0)
            return (null, "Bed dimensions must be positive.");
        if (request.PixelWidth <= 0 || request.PixelHeight <= 0)
            return (null, "Display resolution must be positive.");

        var validationError = ValidateCtbSettings(
            request.LayerHeightMm,
            request.BottomLayerCount,
            request.TransitionLayerCount,
            request.ExposureTimeSeconds,
            request.BottomExposureTimeSeconds,
            request.BottomLiftHeightMm,
            request.BottomLiftSpeedMmPerMinute,
            request.LiftHeightMm,
            request.LiftSpeedMmPerMinute,
            request.RetractSpeedMmPerMinute,
            request.BottomLightOffDelaySeconds,
            request.LightOffDelaySeconds,
            request.WaitTimeBeforeCureSeconds,
            request.WaitTimeAfterCureSeconds,
            request.WaitTimeAfterLiftSeconds,
            request.LightPwm,
            request.BottomLightPwm);
        if (validationError != null)
            return (null, validationError);

        using var db = dbFactory.CreateDbContext();
        var printer = new PrinterConfig
        {
            Id = Guid.NewGuid(),
            Name = name,
            BedWidthMm = request.BedWidthMm,
            BedDepthMm = request.BedDepthMm,
            PixelWidth = request.PixelWidth,
            PixelHeight = request.PixelHeight,
            LayerHeightMm = request.LayerHeightMm,
            BottomLayerCount = request.BottomLayerCount,
            TransitionLayerCount = request.TransitionLayerCount,
            ExposureTimeSeconds = request.ExposureTimeSeconds,
            BottomExposureTimeSeconds = request.BottomExposureTimeSeconds,
            BottomLiftHeightMm = request.BottomLiftHeightMm,
            BottomLiftSpeedMmPerMinute = request.BottomLiftSpeedMmPerMinute,
            LiftHeightMm = request.LiftHeightMm,
            LiftSpeedMmPerMinute = request.LiftSpeedMmPerMinute,
            RetractSpeedMmPerMinute = request.RetractSpeedMmPerMinute,
            BottomLightOffDelaySeconds = request.BottomLightOffDelaySeconds,
            LightOffDelaySeconds = request.LightOffDelaySeconds,
            WaitTimeBeforeCureSeconds = request.WaitTimeBeforeCureSeconds,
            WaitTimeAfterCureSeconds = request.WaitTimeAfterCureSeconds,
            WaitTimeAfterLiftSeconds = request.WaitTimeAfterLiftSeconds,
            LightPwm = request.LightPwm,
            BottomLightPwm = request.BottomLightPwm,
            IsBuiltIn = false,
            IsDefault = false,
        };
        db.PrinterConfigs.Add(printer);
        await db.SaveChangesAsync();
        return (MapDto(printer), null);
    }

    public async Task<(PrinterConfigDto? Dto, string? Error)> UpdateAsync(Guid id, UpdatePrinterConfigRequest request)
    {
        var name = request.Name.Trim();
        if (string.IsNullOrWhiteSpace(name))
            return (null, "Name is required.");
        if (request.BedWidthMm <= 0 || request.BedDepthMm <= 0)
            return (null, "Bed dimensions must be positive.");
        if (request.PixelWidth <= 0 || request.PixelHeight <= 0)
            return (null, "Display resolution must be positive.");

        var validationError = ValidateCtbSettings(
            request.LayerHeightMm,
            request.BottomLayerCount,
            request.TransitionLayerCount,
            request.ExposureTimeSeconds,
            request.BottomExposureTimeSeconds,
            request.BottomLiftHeightMm,
            request.BottomLiftSpeedMmPerMinute,
            request.LiftHeightMm,
            request.LiftSpeedMmPerMinute,
            request.RetractSpeedMmPerMinute,
            request.BottomLightOffDelaySeconds,
            request.LightOffDelaySeconds,
            request.WaitTimeBeforeCureSeconds,
            request.WaitTimeAfterCureSeconds,
            request.WaitTimeAfterLiftSeconds,
            request.LightPwm,
            request.BottomLightPwm);
        if (validationError != null)
            return (null, validationError);

        using var db = dbFactory.CreateDbContext();
        var printer = await db.PrinterConfigs.FindAsync(id);
        if (printer == null) return (null, null);
        if (printer.IsBuiltIn) return (null, "Built-in printers cannot be modified.");

        printer.Name = name;
        printer.BedWidthMm = request.BedWidthMm;
        printer.BedDepthMm = request.BedDepthMm;
        printer.PixelWidth = request.PixelWidth;
        printer.PixelHeight = request.PixelHeight;
        printer.LayerHeightMm = request.LayerHeightMm;
        printer.BottomLayerCount = request.BottomLayerCount;
        printer.TransitionLayerCount = request.TransitionLayerCount;
        printer.ExposureTimeSeconds = request.ExposureTimeSeconds;
        printer.BottomExposureTimeSeconds = request.BottomExposureTimeSeconds;
        printer.BottomLiftHeightMm = request.BottomLiftHeightMm;
        printer.BottomLiftSpeedMmPerMinute = request.BottomLiftSpeedMmPerMinute;
        printer.LiftHeightMm = request.LiftHeightMm;
        printer.LiftSpeedMmPerMinute = request.LiftSpeedMmPerMinute;
        printer.RetractSpeedMmPerMinute = request.RetractSpeedMmPerMinute;
        printer.BottomLightOffDelaySeconds = request.BottomLightOffDelaySeconds;
        printer.LightOffDelaySeconds = request.LightOffDelaySeconds;
        printer.WaitTimeBeforeCureSeconds = request.WaitTimeBeforeCureSeconds;
        printer.WaitTimeAfterCureSeconds = request.WaitTimeAfterCureSeconds;
        printer.WaitTimeAfterLiftSeconds = request.WaitTimeAfterLiftSeconds;
        printer.LightPwm = request.LightPwm;
        printer.BottomLightPwm = request.BottomLightPwm;
        await db.SaveChangesAsync();
        return (MapDto(printer), null);
    }

    private static System.Linq.Expressions.Expression<Func<PrinterConfig, PrinterConfigDto>> MapDtoProjection()
        => p => new PrinterConfigDto(
            p.Id,
            p.Name,
            p.BedWidthMm,
            p.BedDepthMm,
            p.PixelWidth,
            p.PixelHeight,
            p.LayerHeightMm,
            p.BottomLayerCount,
            p.TransitionLayerCount,
            p.ExposureTimeSeconds,
            p.BottomExposureTimeSeconds,
            p.BottomLiftHeightMm,
            p.BottomLiftSpeedMmPerMinute,
            p.LiftHeightMm,
            p.LiftSpeedMmPerMinute,
            p.RetractSpeedMmPerMinute,
            p.BottomLightOffDelaySeconds,
            p.LightOffDelaySeconds,
            p.WaitTimeBeforeCureSeconds,
            p.WaitTimeAfterCureSeconds,
            p.WaitTimeAfterLiftSeconds,
            p.LightPwm,
            p.BottomLightPwm,
            p.IsBuiltIn,
            p.IsDefault);

    private static PrinterConfigDto MapDto(PrinterConfig p)
        => new(
            p.Id,
            p.Name,
            p.BedWidthMm,
            p.BedDepthMm,
            p.PixelWidth,
            p.PixelHeight,
            p.LayerHeightMm,
            p.BottomLayerCount,
            p.TransitionLayerCount,
            p.ExposureTimeSeconds,
            p.BottomExposureTimeSeconds,
            p.BottomLiftHeightMm,
            p.BottomLiftSpeedMmPerMinute,
            p.LiftHeightMm,
            p.LiftSpeedMmPerMinute,
            p.RetractSpeedMmPerMinute,
            p.BottomLightOffDelaySeconds,
            p.LightOffDelaySeconds,
            p.WaitTimeBeforeCureSeconds,
            p.WaitTimeAfterCureSeconds,
            p.WaitTimeAfterLiftSeconds,
            p.LightPwm,
            p.BottomLightPwm,
            p.IsBuiltIn,
            p.IsDefault);

    private static string? ValidateCtbSettings(
        float layerHeightMm,
        int bottomLayerCount,
        int transitionLayerCount,
        float exposureTimeSeconds,
        float bottomExposureTimeSeconds,
        float bottomLiftHeightMm,
        float bottomLiftSpeedMmPerMinute,
        float liftHeightMm,
        float liftSpeedMmPerMinute,
        float retractSpeedMmPerMinute,
        float bottomLightOffDelaySeconds,
        float lightOffDelaySeconds,
        float waitTimeBeforeCureSeconds,
        float waitTimeAfterCureSeconds,
        float waitTimeAfterLiftSeconds,
        byte lightPwm,
        byte bottomLightPwm)
    {
        if (layerHeightMm <= 0)
            return "Layer height must be positive.";
        if (bottomLayerCount < 0)
            return "Bottom layer count cannot be negative.";
        if (transitionLayerCount < 0)
            return "Transition layer count cannot be negative.";
        if (exposureTimeSeconds <= 0 || bottomExposureTimeSeconds <= 0)
            return "Exposure times must be positive.";
        if (bottomLiftHeightMm < 0 || liftHeightMm < 0)
            return "Lift heights cannot be negative.";
        if (bottomLiftSpeedMmPerMinute <= 0 || liftSpeedMmPerMinute <= 0 || retractSpeedMmPerMinute <= 0)
            return "Lift and retract speeds must be positive.";
        if (bottomLightOffDelaySeconds < 0 || lightOffDelaySeconds < 0)
            return "Light-off delays cannot be negative.";
        if (waitTimeBeforeCureSeconds < 0 || waitTimeAfterCureSeconds < 0 || waitTimeAfterLiftSeconds < 0)
            return "Wait times cannot be negative.";
        if (lightPwm == 0 || bottomLightPwm == 0)
            return "Light PWM values must be at least 1.";

        return null;
    }

    public async Task<(bool Found, string? Error)> DeleteAsync(Guid id)
    {
        using var db = dbFactory.CreateDbContext();
        var printer = await db.PrinterConfigs.FindAsync(id);
        if (printer == null) return (false, null);
        if (printer.IsBuiltIn) return (true, "Built-in printers cannot be deleted.");
        if (printer.IsDefault) return (true, "Cannot delete the default printer. Set another printer as default first.");

        db.PrinterConfigs.Remove(printer);
        await db.SaveChangesAsync();
        return (true, null);
    }

    public async Task<(bool Found, string? Error)> SetDefaultAsync(Guid id)
    {
        using var db = dbFactory.CreateDbContext();
        var printer = await db.PrinterConfigs.FindAsync(id);
        if (printer == null) return (false, null);

        var all = await db.PrinterConfigs.ToListAsync();
        foreach (var p in all) p.IsDefault = p.Id == id;
        await db.SaveChangesAsync();
        return (true, null);
    }
}
