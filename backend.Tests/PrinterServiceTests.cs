using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Services;
using Microsoft.EntityFrameworkCore;
using Xunit;

namespace findamodel.Tests;

public class PrinterServiceTests
{
    private static IDbContextFactory<ModelCacheContext> CreateFactory(string dbName)
    {
        var options = new DbContextOptionsBuilder<ModelCacheContext>()
            .UseInMemoryDatabase(dbName)
            .Options;
        return new InMemoryDbContextFactory(options);
    }

    private sealed class InMemoryDbContextFactory(DbContextOptions<ModelCacheContext> options)
        : IDbContextFactory<ModelCacheContext>
    {
        public ModelCacheContext CreateDbContext() => new(options);
        public Task<ModelCacheContext> CreateDbContextAsync(CancellationToken ct = default)
            => Task.FromResult(CreateDbContext());
    }

    [Fact]
    public async Task CreateAsync_CreatesCustomPrinter()
    {
        var sut = new PrinterService(CreateFactory(nameof(CreateAsync_CreatesCustomPrinter)));

        var (dto, error) = await sut.CreateAsync(new("Saturn 4 Ultra", 218, 123, 11520, 5120));

        Assert.Null(error);
        Assert.NotNull(dto);
        Assert.False(dto!.IsBuiltIn);
        Assert.False(dto.IsDefault);
        Assert.Equal("Saturn 4 Ultra", dto.Name);
        Assert.Equal(11520, dto.PixelWidth);
        Assert.Equal(5120, dto.PixelHeight);
    }

    [Fact]
    public async Task DeleteAsync_RejectsBuiltInPrinter()
    {
        var factory = CreateFactory(nameof(DeleteAsync_RejectsBuiltInPrinter));
        await using (var db = factory.CreateDbContext())
        {
            db.PrinterConfigs.Add(new PrinterConfig
            {
                Id = Guid.NewGuid(),
                Name = "Uniformation GK2",
                BedWidthMm = 228,
                BedDepthMm = 128,
                PixelWidth = 7680,
                PixelHeight = 4320,
                IsBuiltIn = true,
                IsDefault = true,
            });
            await db.SaveChangesAsync();
        }

        var sut = new PrinterService(factory);
        await using var read = factory.CreateDbContext();
        var id = await read.PrinterConfigs.Select(p => p.Id).FirstAsync();

        var (found, error) = await sut.DeleteAsync(id);

        Assert.True(found);
        Assert.Equal("Built-in printers cannot be deleted.", error);
    }

    [Fact]
    public async Task SetDefaultAsync_SwitchesDefaultPrinter()
    {
        var factory = CreateFactory(nameof(SetDefaultAsync_SwitchesDefaultPrinter));
        var oldDefaultId = Guid.NewGuid();
        var newDefaultId = Guid.NewGuid();

        await using (var db = factory.CreateDbContext())
        {
            db.PrinterConfigs.AddRange(
                new PrinterConfig
                {
                    Id = oldDefaultId,
                    Name = "Uniformation GK2",
                    BedWidthMm = 228,
                    BedDepthMm = 128,
                    PixelWidth = 7680,
                    PixelHeight = 4320,
                    IsBuiltIn = true,
                    IsDefault = true,
                },
                new PrinterConfig
                {
                    Id = newDefaultId,
                    Name = "Saturn 4 Ultra",
                    BedWidthMm = 218,
                    BedDepthMm = 123,
                    PixelWidth = 11520,
                    PixelHeight = 5120,
                    IsBuiltIn = false,
                    IsDefault = false,
                });
            await db.SaveChangesAsync();
        }

        var sut = new PrinterService(factory);

        var (found, error) = await sut.SetDefaultAsync(newDefaultId);

        Assert.True(found);
        Assert.Null(error);

        await using var verify = factory.CreateDbContext();
        var printers = await verify.PrinterConfigs.OrderBy(p => p.Name).ToListAsync();
        Assert.True(printers.Single(p => p.Id == newDefaultId).IsDefault);
        Assert.False(printers.Single(p => p.Id == oldDefaultId).IsDefault);
    }
}
