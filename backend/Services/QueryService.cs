using Microsoft.EntityFrameworkCore;
using findamodel.Data;
using findamodel.Models;

namespace findamodel.Services;

public class QueryService(IDbContextFactory<ModelCacheContext> dbFactory)
{
    public async Task<ModelQueryResult> QueryModelsAsync(ModelQueryRequest request)
    {
        await using var db = await dbFactory.CreateDbContextAsync();

        var query = db.Models.AsQueryable();

        // Text search: case-insensitive match on file name (without extension)
        if (!string.IsNullOrWhiteSpace(request.Search))
        {
            var term = request.Search.Trim().ToLower();
            query = query.Where(m => m.FileName.ToLower().Contains(term));
        }

        // Multi-value filters on per-model calculated metadata fields (rules applied)
        if (request.Creator is { Length: > 0 })
            query = query.Where(m => m.CalculatedCreator != null && request.Creator.Contains(m.CalculatedCreator));

        if (request.Collection is { Length: > 0 })
            query = query.Where(m => m.CalculatedCollection != null && request.Collection.Contains(m.CalculatedCollection));

        if (request.Subcollection is { Length: > 0 })
            query = query.Where(m => m.CalculatedSubcollection != null && request.Subcollection.Contains(m.CalculatedSubcollection));

        if (request.Category is { Length: > 0 })
            query = query.Where(m => m.CalculatedCategory != null && request.Category.Contains(m.CalculatedCategory));

        if (request.Type is { Length: > 0 })
            query = query.Where(m => m.CalculatedType != null && request.Type.Contains(m.CalculatedType));

        if (request.Material is { Length: > 0 })
            query = query.Where(m => m.CalculatedMaterial != null && request.Material.Contains(m.CalculatedMaterial));

        if (request.FileType is { Length: > 0 })
            query = query.Where(m => request.FileType.Contains(m.FileType));

        // Three-state boolean filter
        if (request.Supported.HasValue)
            query = query.Where(m => m.CalculatedSupported == request.Supported.Value);

        var totalCount = await query.CountAsync();

        var models = await query
            .OrderBy(m => m.Directory)
            .ThenBy(m => m.FileName)
            .Skip(request.Offset)
            .Take(request.Limit)
            .ToListAsync();

        return new ModelQueryResult
        {
            Models = models.Select(m => m.ToModelDto()).ToList(),
            TotalCount = totalCount,
            HasMore = request.Offset + request.Limit < totalCount,
        };
    }

    public async Task<FilterOptionsDto> GetFilterOptionsAsync()
    {
        await using var db = await dbFactory.CreateDbContextAsync();

        var creators = await db.Models
            .Where(m => m.CalculatedCreator != null)
            .Select(m => m.CalculatedCreator!)
            .Distinct()
            .OrderBy(v => v)
            .ToListAsync();

        var collections = await db.Models
            .Where(m => m.CalculatedCollection != null)
            .Select(m => m.CalculatedCollection!)
            .Distinct()
            .OrderBy(v => v)
            .ToListAsync();

        var subcollections = await db.Models
            .Where(m => m.CalculatedSubcollection != null)
            .Select(m => m.CalculatedSubcollection!)
            .Distinct()
            .OrderBy(v => v)
            .ToListAsync();

        var categories = await db.Models
            .Where(m => m.CalculatedCategory != null)
            .Select(m => m.CalculatedCategory!)
            .Distinct()
            .OrderBy(v => v)
            .ToListAsync();

        var types = await db.Models
            .Where(m => m.CalculatedType != null)
            .Select(m => m.CalculatedType!)
            .Distinct()
            .OrderBy(v => v)
            .ToListAsync();

        var materials = await db.Models
            .Where(m => m.CalculatedMaterial != null)
            .Select(m => m.CalculatedMaterial!)
            .Distinct()
            .OrderBy(v => v)
            .ToListAsync();

        var fileTypes = await db.Models
            .Select(m => m.FileType)
            .Distinct()
            .OrderBy(v => v)
            .ToListAsync();

        return new FilterOptionsDto
        {
            Creators = creators,
            Collections = collections,
            Subcollections = subcollections,
            Categories = categories,
            Types = types,
            Materials = materials,
            FileTypes = fileTypes,
        };
    }
}
