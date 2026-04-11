using Microsoft.EntityFrameworkCore;
using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Models;

namespace findamodel.Services;

public class AppConfigService(IDbContextFactory<ModelCacheContext> dbFactory)
{
    private const int SingletonConfigId = 1;
    public const float DatabaseDefaultRaftHeightMm = 2f;

    public async Task<float> GetDefaultRaftHeightMmAsync()
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        var config = await EnsureConfigAsync(db);
        return config.DefaultRaftHeightMm;
    }

    public async Task<AppConfigDto> GetAsync()
    {
        await using var db = await dbFactory.CreateDbContextAsync();
        var config = await EnsureConfigAsync(db);
        return new AppConfigDto(config.DefaultRaftHeightMm);
    }

    public async Task<AppConfigDto> UpdateDefaultRaftHeightAsync(float defaultRaftHeightMm)
    {
        if (!float.IsFinite(defaultRaftHeightMm) || defaultRaftHeightMm < 0f)
            throw new ArgumentException("Default raft height must be a finite number greater than or equal to 0.", nameof(defaultRaftHeightMm));

        await using var db = await dbFactory.CreateDbContextAsync();
        var config = await EnsureConfigAsync(db);
        config.DefaultRaftHeightMm = defaultRaftHeightMm;
        config.UpdatedAt = DateTime.UtcNow;
        await db.SaveChangesAsync();
        return new AppConfigDto(config.DefaultRaftHeightMm);
    }

    private static async Task<AppConfig> EnsureConfigAsync(ModelCacheContext db)
    {
        var config = await db.AppConfigs.FirstOrDefaultAsync(c => c.Id == SingletonConfigId);
        if (config != null)
            return config;

        config = new AppConfig
        {
            Id = SingletonConfigId,
            DefaultRaftHeightMm = DatabaseDefaultRaftHeightMm,
            UpdatedAt = DateTime.UtcNow,
        };

        db.AppConfigs.Add(config);
        await db.SaveChangesAsync();
        return config;
    }
}
