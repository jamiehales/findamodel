using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Models;
using Microsoft.EntityFrameworkCore;

namespace findamodel.Services;

public class PrintingListService(IDbContextFactory<ModelCacheContext> dbFactory)
{
    public async Task EnsureDefaultListAsync(Guid userId)
    {
        using var db = dbFactory.CreateDbContext();
        if (await db.PrintingLists.AnyAsync(l => l.OwnerId == userId && l.IsDefault))
            return;

        // Deactivate any existing lists before making the default one active
        var existing = await db.PrintingLists.Where(l => l.OwnerId == userId).ToListAsync();
        foreach (var l in existing) l.IsActive = false;

        var list = new PrintingList
        {
            Id = Guid.NewGuid(),
            Name = "Default",
            OwnerId = userId,
            IsActive = true,
            IsDefault = true,
            SpawnType = PrintingList.DefaultSpawnType,
            HullMode = PrintingList.DefaultHullMode,
            CreatedAt = DateTime.UtcNow,
        };
        db.PrintingLists.Add(list);
        await db.SaveChangesAsync();
    }

    public async Task<List<PrintingListSummaryDto>> GetListsAsync(Guid userId, bool isAdmin)
    {
        using var db = dbFactory.CreateDbContext();
        var query = db.PrintingLists
            .Include(l => l.Owner)
            .Include(l => l.Items)
            .AsQueryable();

        if (!isAdmin)
            query = query.Where(l => l.OwnerId == userId);

        return await query
            .OrderByDescending(l => l.IsActive)
            .ThenBy(l => l.CreatedAt)
            .Select(l => new PrintingListSummaryDto(
                l.Id, l.Name, l.IsActive, l.IsDefault, l.SpawnType, l.HullMode, l.CreatedAt,
                l.Owner.Username,
                l.Items.Count))
            .ToListAsync();
    }

    public async Task<PrintingListDetailDto?> GetActiveListAsync(Guid userId)
    {
        using var db = dbFactory.CreateDbContext();
        var list = await db.PrintingLists
            .Include(l => l.Owner)
            .Include(l => l.Items)
            .FirstOrDefaultAsync(l => l.OwnerId == userId && l.IsActive);

        return list == null ? null : ToDetail(list, list.Items);
    }

    public async Task<PrintingListDetailDto?> GetListAsync(Guid id, Guid userId, bool isAdmin)
    {
        using var db = dbFactory.CreateDbContext();
        var list = await db.PrintingLists
            .Include(l => l.Owner)
            .Include(l => l.Items)
            .FirstOrDefaultAsync(l => l.Id == id);

        if (list == null) return null;
        if (!isAdmin && list.OwnerId != userId) return null;
        return ToDetail(list, list.Items);
    }

    public async Task<PrintingListSummaryDto> CreateListAsync(Guid userId, string name)
    {
        using var db = dbFactory.CreateDbContext();
        var hasAny = await db.PrintingLists.AnyAsync(l => l.OwnerId == userId);

        var list = new PrintingList
        {
            Id = Guid.NewGuid(),
            Name = name,
            OwnerId = userId,
            IsActive = !hasAny,
            IsDefault = false,
            SpawnType = PrintingList.DefaultSpawnType,
            HullMode = PrintingList.DefaultHullMode,
            CreatedAt = DateTime.UtcNow,
        };
        db.PrintingLists.Add(list);
        await db.SaveChangesAsync();

        await db.Entry(list).Reference(l => l.Owner).LoadAsync();
        return new PrintingListSummaryDto(list.Id, list.Name, list.IsActive, list.IsDefault, list.SpawnType, list.HullMode, list.CreatedAt, list.Owner.Username, 0);
    }

    public async Task<(PrintingListMutateResult Result, PrintingListSummaryDto? Dto)> RenameListAsync(
        Guid id, Guid userId, bool isAdmin, string name)
    {
        using var db = dbFactory.CreateDbContext();
        var list = await db.PrintingLists.Include(l => l.Owner).Include(l => l.Items)
            .FirstOrDefaultAsync(l => l.Id == id);

        if (list == null || (!isAdmin && list.OwnerId != userId))
            return (PrintingListMutateResult.NotFound, null);
        if (list.IsDefault)
            return (PrintingListMutateResult.IsDefault, null);

        list.Name = name;
        await db.SaveChangesAsync();
        return (PrintingListMutateResult.Success,
            new PrintingListSummaryDto(list.Id, list.Name, list.IsActive, list.IsDefault, list.SpawnType, list.HullMode, list.CreatedAt, list.Owner.Username, list.Items.Count));
    }

    public async Task<PrintingListDetailDto?> UpdateSettingsAsync(
        Guid id,
        Guid userId,
        bool isAdmin,
        string spawnType,
        string hullMode)
    {
        using var db = dbFactory.CreateDbContext();
        var list = await db.PrintingLists
            .Include(l => l.Owner)
            .FirstOrDefaultAsync(l => l.Id == id);

        if (list == null) return null;
        if (!isAdmin && list.OwnerId != userId) return null;

        list.SpawnType = NormalizeSpawnType(spawnType);
        list.HullMode = NormalizeHullMode(hullMode);
        await db.SaveChangesAsync();

        var items = await db.PrintingListItems.Where(i => i.PrintingListId == id).ToListAsync();
        return ToDetail(list, items);
    }

    public async Task<PrintingListMutateResult> DeleteListAsync(Guid id, Guid userId, bool isAdmin)
    {
        using var db = dbFactory.CreateDbContext();
        var list = await db.PrintingLists.FirstOrDefaultAsync(l => l.Id == id);
        if (list == null || (!isAdmin && list.OwnerId != userId))
            return PrintingListMutateResult.NotFound;
        if (list.IsDefault)
            return PrintingListMutateResult.IsDefault;

        var wasActive = list.IsActive;
        var ownerId = list.OwnerId;

        db.PrintingLists.Remove(list);
        await db.SaveChangesAsync();

        if (wasActive)
        {
            var next = await db.PrintingLists
                .Where(l => l.OwnerId == ownerId)
                .OrderBy(l => l.CreatedAt)
                .FirstOrDefaultAsync();
            if (next != null)
            {
                next.IsActive = true;
                await db.SaveChangesAsync();
            }
        }

        return PrintingListMutateResult.Success;
    }

    public async Task<Guid?> ResolveListIdAsync(string id, Guid userId)
    {
        if (id.Equals("active", StringComparison.OrdinalIgnoreCase))
        {
            using var db = dbFactory.CreateDbContext();
            var list = await db.PrintingLists.FirstOrDefaultAsync(l => l.OwnerId == userId && l.IsActive);
            return list?.Id;
        }
        return Guid.TryParse(id, out var guid) ? guid : null;
    }

    public async Task<bool> ActivateListAsync(Guid id, Guid userId, bool isAdmin)
    {
        using var db = dbFactory.CreateDbContext();
        var list = await db.PrintingLists.FirstOrDefaultAsync(l => l.Id == id);
        if (list == null) return false;
        if (!isAdmin && list.OwnerId != userId) return false;

        var ownerLists = await db.PrintingLists
            .Where(l => l.OwnerId == list.OwnerId)
            .ToListAsync();
        foreach (var l in ownerLists) l.IsActive = false;

        list.IsActive = true;
        await db.SaveChangesAsync();
        return true;
    }

    public async Task<PrintingListDetailDto?> UpsertItemAsync(Guid listId, Guid userId, bool isAdmin, Guid modelId, int quantity)
    {
        using var db = dbFactory.CreateDbContext();
        var list = await db.PrintingLists.Include(l => l.Owner)
            .FirstOrDefaultAsync(l => l.Id == listId);

        if (list == null) return null;
        if (!isAdmin && list.OwnerId != userId) return null;

        // Work through the DbSet directly to avoid navigation-collection change-tracking issues
        var item = await db.PrintingListItems
            .FirstOrDefaultAsync(i => i.PrintingListId == listId && i.ModelId == modelId);

        if (quantity <= 0)
        {
            if (item != null) db.PrintingListItems.Remove(item);
        }
        else if (item == null)
        {
            db.PrintingListItems.Add(new PrintingListItem
            {
                Id = Guid.NewGuid(),
                PrintingListId = listId,
                ModelId = modelId,
                Quantity = quantity,
            });
        }
        else
        {
            item.Quantity = quantity;
        }

        await db.SaveChangesAsync();

        var items = await db.PrintingListItems.Where(i => i.PrintingListId == listId).ToListAsync();
        return ToDetail(list, items);
    }

    public async Task<PrintingListDetailDto?> ClearItemsAsync(Guid listId, Guid userId, bool isAdmin)
    {
        using var db = dbFactory.CreateDbContext();
        var list = await db.PrintingLists.Include(l => l.Owner)
            .FirstOrDefaultAsync(l => l.Id == listId);

        if (list == null) return null;
        if (!isAdmin && list.OwnerId != userId) return null;

        await db.PrintingListItems.Where(i => i.PrintingListId == listId).ExecuteDeleteAsync();
        return ToDetail(list, []);
    }

    private static PrintingListDetailDto ToDetail(PrintingList list, IEnumerable<PrintingListItem> items) =>
        new(list.Id, list.Name, list.IsActive, list.IsDefault, NormalizeSpawnType(list.SpawnType), NormalizeHullMode(list.HullMode), list.CreatedAt, list.Owner.Username,
            items.Select(i => new PrintingListItemDto(i.Id, i.ModelId, i.Quantity)).ToList());

    private static string NormalizeSpawnType(string? spawnType) =>
        spawnType?.Trim().ToLowerInvariant() switch
        {
            "random" => "random",
            "largestfirstfillgaps" => "largestFirstFillGaps",
            _ => PrintingList.DefaultSpawnType,
        };

    private static string NormalizeHullMode(string? hullMode) =>
        string.Equals(hullMode, "sansRaft", StringComparison.OrdinalIgnoreCase) ? "sansRaft" : PrintingList.DefaultHullMode;
}
