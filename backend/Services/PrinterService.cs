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
            .Select(p => new PrinterConfigDto(p.Id, p.Name, p.BedWidthMm, p.BedDepthMm, p.IsBuiltIn, p.IsDefault))
            .ToListAsync();
    }

    public async Task<PrinterConfigDto?> GetDefaultAsync()
    {
        using var db = dbFactory.CreateDbContext();
        var p = await db.PrinterConfigs.FirstOrDefaultAsync(p => p.IsDefault);
        return p == null ? null : new PrinterConfigDto(p.Id, p.Name, p.BedWidthMm, p.BedDepthMm, p.IsBuiltIn, p.IsDefault);
    }

    public async Task<(PrinterConfigDto? Dto, string? Error)> CreateAsync(CreatePrinterConfigRequest request)
    {
        var (name, width, depth) = (request.Name.Trim(), request.BedWidthMm, request.BedDepthMm);
        if (string.IsNullOrWhiteSpace(name))
            return (null, "Name is required.");
        if (width <= 0 || depth <= 0)
            return (null, "Bed dimensions must be positive.");

        using var db = dbFactory.CreateDbContext();
        var printer = new PrinterConfig
        {
            Id = Guid.NewGuid(),
            Name = name,
            BedWidthMm = width,
            BedDepthMm = depth,
            IsBuiltIn = false,
            IsDefault = false,
        };
        db.PrinterConfigs.Add(printer);
        await db.SaveChangesAsync();
        return (new PrinterConfigDto(printer.Id, printer.Name, printer.BedWidthMm, printer.BedDepthMm, false, false), null);
    }

    public async Task<(PrinterConfigDto? Dto, string? Error)> UpdateAsync(Guid id, UpdatePrinterConfigRequest request)
    {
        var (name, width, depth) = (request.Name.Trim(), request.BedWidthMm, request.BedDepthMm);
        if (string.IsNullOrWhiteSpace(name))
            return (null, "Name is required.");
        if (width <= 0 || depth <= 0)
            return (null, "Bed dimensions must be positive.");

        using var db = dbFactory.CreateDbContext();
        var printer = await db.PrinterConfigs.FindAsync(id);
        if (printer == null) return (null, null);
        if (printer.IsBuiltIn) return (null, "Built-in printers cannot be modified.");

        printer.Name = name;
        printer.BedWidthMm = width;
        printer.BedDepthMm = depth;
        await db.SaveChangesAsync();
        return (new PrinterConfigDto(printer.Id, printer.Name, printer.BedWidthMm, printer.BedDepthMm, printer.IsBuiltIn, printer.IsDefault), null);
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
