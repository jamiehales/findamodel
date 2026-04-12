using Microsoft.Data.Sqlite;
using Microsoft.EntityFrameworkCore;
using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Models;
using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

public class QueryServiceTests
{
    private sealed class SqliteDbContextFactory(DbContextOptions<ModelCacheContext> options)
        : IDbContextFactory<ModelCacheContext>
    {
        public ModelCacheContext CreateDbContext() => new(options);

        public Task<ModelCacheContext> CreateDbContextAsync(CancellationToken ct = default) =>
            Task.FromResult(CreateDbContext());
    }

    private static (IDbContextFactory<ModelCacheContext> Factory, SqliteConnection Connection) CreateFactory()
    {
        var connection = new SqliteConnection("Data Source=:memory:");
        connection.Open();

        var options = new DbContextOptionsBuilder<ModelCacheContext>()
            .UseSqlite(connection)
            .Options;

        using var db = new ModelCacheContext(options);
        db.Database.EnsureCreated();

        return (new SqliteDbContextFactory(options), connection);
    }

    [Fact]
    public async Task QueryModelsAsync_WithTagsFilter_MatchesAnySelectedTag()
    {
        var (factory, connection) = CreateFactory();
        try
        {
            await using (var db = await factory.CreateDbContextAsync())
            {
                db.Models.AddRange(
                    new CachedModel
                    {
                        Id = Guid.NewGuid(),
                        FileName = "dragon.stl",
                        Directory = "fantasy",
                        FileType = "stl",
                        Checksum = "a",
                        FileSize = 10,
                        FileModifiedAt = DateTime.UtcNow,
                        CachedAt = DateTime.UtcNow,
                        CalculatedTagsJson = "[\"monster\",\"small\"]",
                    },
                    new CachedModel
                    {
                        Id = Guid.NewGuid(),
                        FileName = "tank.stl",
                        Directory = "scifi",
                        FileType = "stl",
                        Checksum = "b",
                        FileSize = 20,
                        FileModifiedAt = DateTime.UtcNow,
                        CachedAt = DateTime.UtcNow,
                        CalculatedTagsJson = "[\"vehicle\",\"large\"]",
                    },
                    new CachedModel
                    {
                        Id = Guid.NewGuid(),
                        FileName = "base.stl",
                        Directory = "bits",
                        FileType = "stl",
                        Checksum = "c",
                        FileSize = 5,
                        FileModifiedAt = DateTime.UtcNow,
                        CachedAt = DateTime.UtcNow,
                        CalculatedTagsJson = "[\"small\",\"terrain\"]",
                    });

                await db.SaveChangesAsync();
            }

            var sut = new QueryService(factory);
            var result = await sut.QueryModelsAsync(new ModelQueryRequest
            {
                Tags = ["monster", "vehicle"],
                Limit = 50,
                Offset = 0,
            });

            Assert.Equal(2, result.TotalCount);
            Assert.False(result.HasMore);
            Assert.Contains(result.Models, m => string.Equals(m.Name, "dragon", StringComparison.OrdinalIgnoreCase));
            Assert.Contains(result.Models, m => string.Equals(m.Name, "tank", StringComparison.OrdinalIgnoreCase));
            Assert.DoesNotContain(result.Models, m => string.Equals(m.Name, "base", StringComparison.OrdinalIgnoreCase));
        }
        finally
        {
            await connection.DisposeAsync();
        }
    }

    [Fact]
    public async Task GetFilterOptionsAsync_IncludesDistinctTags()
    {
        var (factory, connection) = CreateFactory();
        try
        {
            await using (var db = await factory.CreateDbContextAsync())
            {
                db.Models.AddRange(
                    new CachedModel
                    {
                        Id = Guid.NewGuid(),
                        FileName = "one.stl",
                        Directory = "a",
                        FileType = "stl",
                        Checksum = "1",
                        FileSize = 1,
                        FileModifiedAt = DateTime.UtcNow,
                        CachedAt = DateTime.UtcNow,
                        CalculatedTagsJson = "[\"small\",\"monster\"]",
                        GeneratedTagsJson = "[\"printed\"]",
                    },
                    new CachedModel
                    {
                        Id = Guid.NewGuid(),
                        FileName = "two.stl",
                        Directory = "b",
                        FileType = "stl",
                        Checksum = "2",
                        FileSize = 1,
                        FileModifiedAt = DateTime.UtcNow,
                        CachedAt = DateTime.UtcNow,
                        CalculatedTagsJson = "[\"large\",\"monster\"]",
                        GeneratedTagsJson = "[\"boss\"]",
                    });

                await db.SaveChangesAsync();
            }

            var sut = new QueryService(factory);
            var options = await sut.GetFilterOptionsAsync();

            Assert.Contains("large", options.Tags);
            Assert.Contains("printed", options.Tags);
            Assert.Contains("boss", options.Tags);
            Assert.Contains("monster", options.Tags);
            Assert.Contains("small", options.Tags);
            Assert.Equal(5, options.Tags.Count);
        }
        finally
        {
            await connection.DisposeAsync();
        }
    }

    [Fact]
    public async Task QueryModelsAsync_SearchMatchesGeneratedDescription()
    {
        var (factory, connection) = CreateFactory();
        try
        {
            await using (var db = await factory.CreateDbContextAsync())
            {
                db.Models.AddRange(
                    new CachedModel
                    {
                        Id = Guid.NewGuid(),
                        FileName = "orc-captain.stl",
                        Directory = "warband",
                        FileType = "stl",
                        Checksum = "d",
                        FileSize = 10,
                        FileModifiedAt = DateTime.UtcNow,
                        CachedAt = DateTime.UtcNow,
                        GeneratedDescription = "Heavily armored commander carrying a banner and axe.",
                    },
                    new CachedModel
                    {
                        Id = Guid.NewGuid(),
                        FileName = "village-house.stl",
                        Directory = "terrain",
                        FileType = "stl",
                        Checksum = "e",
                        FileSize = 12,
                        FileModifiedAt = DateTime.UtcNow,
                        CachedAt = DateTime.UtcNow,
                        GeneratedDescription = "Small rustic house with timber roof.",
                    });

                await db.SaveChangesAsync();
            }

            var sut = new QueryService(factory);
            var result = await sut.QueryModelsAsync(new ModelQueryRequest
            {
                Search = "banner",
                Limit = 25,
                Offset = 0,
            });

            Assert.Single(result.Models);
            Assert.Equal("orc-captain", result.Models[0].Name);
        }
        finally
        {
            await connection.DisposeAsync();
        }
    }
}