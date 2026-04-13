using Microsoft.EntityFrameworkCore;
using findamodel.Data;
using findamodel.Models;

namespace findamodel.Services;

public class QueryService(IDbContextFactory<ModelCacheContext> dbFactory)
{
    public async Task<ModelQueryResult> QueryModelsAsync(ModelQueryRequest request)
    {
        await using var db = await dbFactory.CreateDbContextAsync();

        var query = BuildFilteredQuery(db.Models.AsNoTracking(), request);

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

    public async Task<FilterOptionsDto> GetFilterOptionsAsync(ModelQueryRequest request)
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        var models = db.Models.AsNoTracking();

        var creators = await BuildFilteredQuery(models, request, includeCreator: false)
            .Where(m => m.CalculatedCreator != null)
            .Select(m => m.CalculatedCreator!)
            .Distinct()
            .OrderBy(v => v)
            .ToListAsync();
        creators = MergeSelectedOptions(creators, request.Creator);

        var collections = await BuildFilteredQuery(models, request, includeCollection: false)
            .Where(m => m.CalculatedCollection != null)
            .Select(m => m.CalculatedCollection!)
            .Distinct()
            .OrderBy(v => v)
            .ToListAsync();
        collections = MergeSelectedOptions(collections, request.Collection);

        var subcollections = await BuildFilteredQuery(models, request, includeSubcollection: false)
            .Where(m => m.CalculatedSubcollection != null)
            .Select(m => m.CalculatedSubcollection!)
            .Distinct()
            .OrderBy(v => v)
            .ToListAsync();
        subcollections = MergeSelectedOptions(subcollections, request.Subcollection);

        var tags = (await BuildFilteredQuery(models, request, includeTags: false)
                .Where(m => m.CalculatedTagsJson != null || m.GeneratedTagsJson != null)
                .Select(m => new { m.CalculatedTagsJson, m.GeneratedTagsJson })
                .ToListAsync())
            .SelectMany(v => new[] { v.CalculatedTagsJson, v.GeneratedTagsJson })
            .Where(v => !string.IsNullOrWhiteSpace(v))
            .SelectMany(v => TagListHelper.FromJson(v))
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .OrderBy(v => v, StringComparer.OrdinalIgnoreCase)
            .ToList();
        tags = TagListHelper.Merge(tags, request.Tags);

        var categories = await BuildFilteredQuery(models, request, includeCategory: false)
            .Where(m => m.CalculatedCategory != null)
            .Select(m => m.CalculatedCategory!)
            .Distinct()
            .OrderBy(v => v)
            .ToListAsync();
        categories = MergeSelectedOptions(categories, request.Category);

        var types = await BuildFilteredQuery(models, request, includeType: false)
            .Where(m => m.CalculatedType != null)
            .Select(m => m.CalculatedType!)
            .Distinct()
            .OrderBy(v => v)
            .ToListAsync();
        types = MergeSelectedOptions(types, request.Type);

        var materials = await BuildFilteredQuery(models, request, includeMaterial: false)
            .Where(m => m.CalculatedMaterial != null)
            .Select(m => m.CalculatedMaterial!)
            .Distinct()
            .OrderBy(v => v)
            .ToListAsync();
        materials = MergeSelectedOptions(materials, request.Material);

        var fileTypes = await BuildFilteredQuery(models, request, includeFileType: false)
            .Select(m => m.FileType)
            .Distinct()
            .OrderBy(v => v)
            .ToListAsync();
        fileTypes = MergeSelectedOptions(fileTypes, request.FileType);

        return new FilterOptionsDto
        {
            Creators = creators,
            Collections = collections,
            Subcollections = subcollections,
            Tags = tags,
            Categories = categories,
            Types = types,
            Materials = materials,
            FileTypes = fileTypes,
        };
    }

    private static IQueryable<Data.Entities.CachedModel> BuildFilteredQuery(
        IQueryable<Data.Entities.CachedModel> query,
        ModelQueryRequest request,
        bool includeCreator = true,
        bool includeCollection = true,
        bool includeSubcollection = true,
        bool includeTags = true,
        bool includeCategory = true,
        bool includeType = true,
        bool includeMaterial = true,
        bool includeFileType = true)
    {
        // Text search: case-insensitive match on file name (without extension)
        if (!string.IsNullOrWhiteSpace(request.Search))
        {
            var term = request.Search.Trim().ToLower();
            query = query.Where(m =>
                m.FileName.ToLower().Contains(term)
                || (m.CalculatedModelName != null && m.CalculatedModelName.ToLower().Contains(term))
                || (m.GeneratedDescription != null && m.GeneratedDescription.ToLower().Contains(term)));
        }

        // Dedicated model-name filter that combines with text search and all other filters.
        if (!string.IsNullOrWhiteSpace(request.ModelName))
        {
            var modelNameTerm = request.ModelName.Trim().ToLower();
            query = query.Where(m =>
                (m.CalculatedModelName != null && m.CalculatedModelName.ToLower().Contains(modelNameTerm))
                || m.FileName.ToLower().Contains(modelNameTerm));
        }

        // Multi-value filters on per-model calculated metadata fields (rules applied)
        if (includeCreator && request.Creator is { Length: > 0 })
            query = query.Where(m => m.CalculatedCreator != null && request.Creator.Contains(m.CalculatedCreator));

        if (includeCollection && request.Collection is { Length: > 0 })
            query = query.Where(m => m.CalculatedCollection != null && request.Collection.Contains(m.CalculatedCollection));

        if (includeSubcollection && request.Subcollection is { Length: > 0 })
            query = query.Where(m => m.CalculatedSubcollection != null && request.Subcollection.Contains(m.CalculatedSubcollection));

        if (includeCategory && request.Category is { Length: > 0 })
            query = query.Where(m => m.CalculatedCategory != null && request.Category.Contains(m.CalculatedCategory));

        if (includeType && request.Type is { Length: > 0 })
            query = query.Where(m => m.CalculatedType != null && request.Type.Contains(m.CalculatedType));

        if (includeMaterial && request.Material is { Length: > 0 })
            query = query.Where(m => m.CalculatedMaterial != null && request.Material.Contains(m.CalculatedMaterial));

        if (includeFileType && request.FileType is { Length: > 0 })
            query = query.Where(m => request.FileType.Contains(m.FileType));

        if (includeTags)
        {
            var normalizedRequestedTags = TagListHelper.Normalize(request.Tags ?? []);
            if (normalizedRequestedTags.Count > 0)
            {
                var baseQuery = query;
                var tagged = baseQuery.Where(_ => false);

                foreach (var tag in normalizedRequestedTags)
                {
                    var pattern = BuildJsonTagLikePattern(tag);
                    tagged = tagged.Union(baseQuery.Where(m =>
                        (m.CalculatedTagsJson != null && EF.Functions.Like(m.CalculatedTagsJson, pattern))
                        || (m.GeneratedTagsJson != null && EF.Functions.Like(m.GeneratedTagsJson, pattern))));
                }

                query = tagged;
            }
        }

        // Three-state boolean filter
        if (request.Supported.HasValue)
            query = query.Where(m => m.CalculatedSupported == request.Supported.Value);

        return query;
    }

    private static List<string> MergeSelectedOptions(List<string> options, IEnumerable<string>? selected)
    {
        if (selected == null)
            return options;

        var selectedValues = selected
            .Where(v => !string.IsNullOrWhiteSpace(v))
            .Select(v => v.Trim())
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToList();

        if (selectedValues.Count == 0)
            return options;

        return options
            .Concat(selectedValues)
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .OrderBy(v => v, StringComparer.OrdinalIgnoreCase)
            .ToList();
    }

    private static string BuildJsonTagLikePattern(string tag)
    {
        // Tags are stored as JSON string arrays; match a complete quoted token.
        var escaped = tag.Replace("\"", "\\\"");
        return $"%\"{escaped}\"%";
    }
}
