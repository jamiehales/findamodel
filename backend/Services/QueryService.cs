using Microsoft.EntityFrameworkCore;
using findamodel.Data;
using findamodel.Models;

namespace findamodel.Services;

public class QueryService(IDbContextFactory<ModelCacheContext> dbFactory)
{
    public async Task<ModelQueryResult> QueryModelsAsync(ModelQueryRequest request)
    {
        await using var db = await dbFactory.CreateDbContextAsync();

        var query = db.Models
            .Include(m => m.DirectoryConfig)
            .AsQueryable();

        // Text search: case-insensitive match on file name (without extension)
        if (!string.IsNullOrWhiteSpace(request.Search))
        {
            var term = request.Search.Trim().ToLower();
            query = query.Where(m => m.FileName.ToLower().Contains(term));
        }

        // Multi-value filters on resolved DirectoryConfig fields
        if (request.Creator is { Length: > 0 })
            query = query.Where(m => m.DirectoryConfig != null && request.Creator.Contains(m.DirectoryConfig.Creator));

        if (request.Collection is { Length: > 0 })
            query = query.Where(m => m.DirectoryConfig != null && request.Collection.Contains(m.DirectoryConfig.Collection));

        if (request.Subcollection is { Length: > 0 })
            query = query.Where(m => m.DirectoryConfig != null && request.Subcollection.Contains(m.DirectoryConfig.Subcollection));

        if (request.Category is { Length: > 0 })
            query = query.Where(m => m.DirectoryConfig != null && request.Category.Contains(m.DirectoryConfig.Category));

        if (request.Type is { Length: > 0 })
            query = query.Where(m => m.DirectoryConfig != null && request.Type.Contains(m.DirectoryConfig.Type));

        if (request.FileType is { Length: > 0 })
            query = query.Where(m => request.FileType.Contains(m.FileType));

        // Three-state boolean filter
        if (request.Supported.HasValue)
            query = query.Where(m => m.DirectoryConfig != null && m.DirectoryConfig.Supported == request.Supported.Value);

        var totalCount = await query.CountAsync();

        var models = await query
            .OrderBy(m => m.Directory)
            .ThenBy(m => m.FileName)
            .Skip(request.Offset)
            .Take(request.Limit)
            .ToListAsync();

        return new ModelQueryResult
        {
            Models = models.Select(m => new ModelDto
            {
                Id            = m.Id,
                Name          = Path.GetFileNameWithoutExtension(m.FileName),
                RelativePath  = string.IsNullOrEmpty(m.Directory) ? m.FileName : $"{m.Directory}/{m.FileName}",
                FileType      = m.FileType,
                FileSize      = m.FileSize,
                FileUrl       = $"/api/models/{m.Id}/file",
                HasPreview    = m.PreviewImagePath != null,
                PreviewUrl    = m.PreviewImagePath != null ? $"/api/models/{m.Id}/preview" : null,
                Creator        = m.DirectoryConfig?.Creator,
                Collection    = m.DirectoryConfig?.Collection,
                Subcollection = m.DirectoryConfig?.Subcollection,
                Category      = m.DirectoryConfig?.Category,
                Type          = m.DirectoryConfig?.Type,
                Supported     = m.DirectoryConfig?.Supported,
                ConvexHull         = m.ConvexHullCoordinates,
                ConcaveHull        = m.ConcaveHullCoordinates,
                ConvexSansRaftHull = m.ConvexSansRaftHullCoordinates,
                DimensionXMm  = m.DimensionXMm,
                DimensionYMm  = m.DimensionYMm,
                DimensionZMm  = m.DimensionZMm,
                SphereCentreX = m.SphereCentreX,
                SphereCentreY = m.SphereCentreY,
                SphereCentreZ = m.SphereCentreZ,
                SphereRadius  = m.SphereRadius,
            }).ToList(),
            TotalCount = totalCount,
            HasMore = request.Offset + request.Limit < totalCount,
        };
    }

    public async Task<FilterOptionsDto> GetFilterOptionsAsync()
    {
        await using var db = await dbFactory.CreateDbContextAsync();

        var creators = await db.Models
            .Where(m => m.DirectoryConfig != null && m.DirectoryConfig.Creator != null)
            .Select(m => m.DirectoryConfig!.Creator!)
            .Distinct()
            .OrderBy(v => v)
            .ToListAsync();

        var collections = await db.Models
            .Where(m => m.DirectoryConfig != null && m.DirectoryConfig.Collection != null)
            .Select(m => m.DirectoryConfig!.Collection!)
            .Distinct()
            .OrderBy(v => v)
            .ToListAsync();

        var subcollections = await db.Models
            .Where(m => m.DirectoryConfig != null && m.DirectoryConfig.Subcollection != null)
            .Select(m => m.DirectoryConfig!.Subcollection!)
            .Distinct()
            .OrderBy(v => v)
            .ToListAsync();

        var categories = await db.Models
            .Where(m => m.DirectoryConfig != null && m.DirectoryConfig.Category != null)
            .Select(m => m.DirectoryConfig!.Category!)
            .Distinct()
            .OrderBy(v => v)
            .ToListAsync();

        var types = await db.Models
            .Where(m => m.DirectoryConfig != null && m.DirectoryConfig.Type != null)
            .Select(m => m.DirectoryConfig!.Type!)
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
            Creators      = creators,
            Collections   = collections,
            Subcollections = subcollections,
            Categories    = categories,
            Types         = types,
            FileTypes     = fileTypes,
        };
    }
}
