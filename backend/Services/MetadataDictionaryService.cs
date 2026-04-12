using Microsoft.EntityFrameworkCore;
using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Models;

namespace findamodel.Services;

public class MetadataDictionaryService(IDbContextFactory<ModelCacheContext> dbFactory)
{
    private static readonly HashSet<string> AllowedFields =
        new(["category", "type", "material", "tags"], StringComparer.OrdinalIgnoreCase);

    public static bool IsSupportedField(string field) => AllowedFields.Contains(field);

    public async Task<MetadataDictionaryOverviewDto> GetOverviewAsync()
    {
        await using var db = await dbFactory.CreateDbContextAsync();

        var configured = await db.MetadataDictionaryValues
            .AsNoTracking()
            .Where(v => v.Field == "category" || v.Field == "type" || v.Field == "material" || v.Field == "tags")
            .OrderBy(v => v.Field)
            .ThenBy(v => v.Value)
            .ToListAsync();

        var categoryFromModels = await db.Models
            .AsNoTracking()
            .Where(m => m.CalculatedCategory != null && m.CalculatedCategory != "")
            .Select(m => m.CalculatedCategory!)
            .Distinct()
            .ToListAsync();

        var rawCategoryFromDirectories = await db.DirectoryConfigs
            .AsNoTracking()
            .Where(d => d.RawCategory != null && d.RawCategory != "")
            .Select(d => d.RawCategory!)
            .Distinct()
            .ToListAsync();

        var resolvedCategoryFromDirectories = await db.DirectoryConfigs
            .AsNoTracking()
            .Where(d => d.Category != null && d.Category != "")
            .Select(d => d.Category!)
            .Distinct()
            .ToListAsync();

        var observedCategories = categoryFromModels
            .Union(rawCategoryFromDirectories)
            .Union(resolvedCategoryFromDirectories)
            .OrderBy(v => v)
            .ToList();

        var typeFromModels = await db.Models
            .AsNoTracking()
            .Where(m => m.CalculatedType != null && m.CalculatedType != "")
            .Select(m => m.CalculatedType!)
            .Distinct()
            .ToListAsync();

        var rawTypeFromDirectories = await db.DirectoryConfigs
            .AsNoTracking()
            .Where(d => d.RawType != null && d.RawType != "")
            .Select(d => d.RawType!)
            .Distinct()
            .ToListAsync();

        var resolvedTypeFromDirectories = await db.DirectoryConfigs
            .AsNoTracking()
            .Where(d => d.Type != null && d.Type != "")
            .Select(d => d.Type!)
            .Distinct()
            .ToListAsync();

        var observedTypes = typeFromModels
            .Union(rawTypeFromDirectories)
            .Union(resolvedTypeFromDirectories)
            .OrderBy(v => v)
            .ToList();

        var materialFromModels = await db.Models
            .AsNoTracking()
            .Where(m => m.CalculatedMaterial != null && m.CalculatedMaterial != "")
            .Select(m => m.CalculatedMaterial!)
            .Distinct()
            .ToListAsync();

        var rawMaterialFromDirectories = await db.DirectoryConfigs
            .AsNoTracking()
            .Where(d => d.RawMaterial != null && d.RawMaterial != "")
            .Select(d => d.RawMaterial!)
            .Distinct()
            .ToListAsync();

        var resolvedMaterialFromDirectories = await db.DirectoryConfigs
            .AsNoTracking()
            .Where(d => d.Material != null && d.Material != "")
            .Select(d => d.Material!)
            .Distinct()
            .ToListAsync();

        var observedMaterials = materialFromModels
            .Union(rawMaterialFromDirectories)
            .Union(resolvedMaterialFromDirectories)
            .OrderBy(v => v)
            .ToList();

        var modelTags = (await db.Models
                .AsNoTracking()
                .Where(m => m.CalculatedTagsJson != null)
                .Select(m => m.CalculatedTagsJson)
                .ToListAsync())
            .Where(v => !string.IsNullOrWhiteSpace(v))
            .SelectMany(v => TagListHelper.FromJson(v));

        var rawDirectoryTags = (await db.DirectoryConfigs
                .AsNoTracking()
                .Where(d => d.RawTagsJson != null)
                .Select(d => d.RawTagsJson)
                .ToListAsync())
            .Where(v => !string.IsNullOrWhiteSpace(v))
            .SelectMany(v => TagListHelper.FromJson(v));

        var resolvedDirectoryTags = (await db.DirectoryConfigs
                .AsNoTracking()
                .Where(d => d.TagsJson != null)
                .Select(d => d.TagsJson)
                .ToListAsync())
            .Where(v => !string.IsNullOrWhiteSpace(v))
            .SelectMany(v => TagListHelper.FromJson(v));

        var observedTags = modelTags
            .Union(rawDirectoryTags, StringComparer.OrdinalIgnoreCase)
            .Union(resolvedDirectoryTags, StringComparer.OrdinalIgnoreCase)
            .OrderBy(v => v, StringComparer.OrdinalIgnoreCase)
            .ToList();

        return new MetadataDictionaryOverviewDto(
            BuildField("category", configured, observedCategories),
            BuildField("type", configured, observedTypes),
            BuildField("material", configured, observedMaterials),
            BuildField("tags", configured, observedTags));
    }

    public async Task<MetadataDictionaryValueDto> CreateAsync(string field, string value)
    {
        var normalizedField = NormalizeField(field);
        var normalizedValue = NormalizeValue(value);

        await using var db = await dbFactory.CreateDbContextAsync();

        var existing = await db.MetadataDictionaryValues
            .FirstOrDefaultAsync(v =>
                v.Field == normalizedField &&
                v.NormalizedValue == normalizedValue);

        if (existing != null)
            return new MetadataDictionaryValueDto(existing.Id, existing.Value);

        var entity = new MetadataDictionaryValue
        {
            Id = Guid.NewGuid(),
            Field = normalizedField,
            Value = value.Trim(),
            NormalizedValue = normalizedValue,
            CreatedAt = DateTime.UtcNow,
            UpdatedAt = DateTime.UtcNow,
        };

        db.MetadataDictionaryValues.Add(entity);
        await db.SaveChangesAsync();

        return new MetadataDictionaryValueDto(entity.Id, entity.Value);
    }

    public async Task<MetadataDictionaryValueDto?> UpdateAsync(Guid id, string value)
    {
        var normalizedValue = NormalizeValue(value);

        await using var db = await dbFactory.CreateDbContextAsync();
        var entity = await db.MetadataDictionaryValues.FirstOrDefaultAsync(v => v.Id == id);
        if (entity == null) return null;

        var conflict = await db.MetadataDictionaryValues.AnyAsync(v =>
            v.Id != id &&
            v.Field == entity.Field &&
            v.NormalizedValue == normalizedValue);

        if (conflict)
            throw new InvalidOperationException($"Value '{value.Trim()}' already exists for {entity.Field}.");

        entity.Value = value.Trim();
        entity.NormalizedValue = normalizedValue;
        entity.UpdatedAt = DateTime.UtcNow;

        await db.SaveChangesAsync();
        return new MetadataDictionaryValueDto(entity.Id, entity.Value);
    }

    public async Task<bool> DeleteAsync(Guid id)
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        var entity = await db.MetadataDictionaryValues.FirstOrDefaultAsync(v => v.Id == id);
        if (entity == null) return false;

        db.MetadataDictionaryValues.Remove(entity);
        await db.SaveChangesAsync();
        return true;
    }

    private static MetadataDictionaryFieldDto BuildField(
        string field,
        List<MetadataDictionaryValue> configured,
        List<string> observed)
    {
        var configuredValues = configured
            .Where(v => string.Equals(v.Field, field, StringComparison.OrdinalIgnoreCase))
            .Select(v => new MetadataDictionaryValueDto(v.Id, v.Value))
            .ToList();

        return new MetadataDictionaryFieldDto(configuredValues, observed);
    }

    private static string NormalizeField(string field)
    {
        var normalized = field.Trim().ToLowerInvariant();
        if (!AllowedFields.Contains(normalized))
            throw new ArgumentException($"Unsupported field '{field}'.", nameof(field));
        return normalized;
    }

    private static string NormalizeValue(string value)
    {
        var trimmed = value.Trim();
        if (string.IsNullOrWhiteSpace(trimmed))
            throw new ArgumentException("Value cannot be empty.", nameof(value));
        return trimmed.ToLowerInvariant();
    }
}
