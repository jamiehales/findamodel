using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Services;
using Microsoft.EntityFrameworkCore;
using Xunit;

namespace findamodel.Tests;

public class PrintingListPrinterTests
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
    public async Task CreateListAsync_AssignsDefaultPrinter()
    {
        var factory = CreateFactory(nameof(CreateListAsync_AssignsDefaultPrinter));
        var userId = Guid.NewGuid();
        var printerId = Guid.NewGuid();

        await using (var db = factory.CreateDbContext())
        {
            db.Users.Add(new User { Id = userId, Username = "jamie", IsAdmin = false });
            db.PrinterConfigs.Add(new PrinterConfig
            {
                Id = printerId,
                Name = "Uniformation GK2",
                BedWidthMm = 228,
                BedDepthMm = 128,
                IsBuiltIn = true,
                IsDefault = true,
            });
            await db.SaveChangesAsync();
        }

        var sut = new PrintingListService(factory);
        var created = await sut.CreateListAsync(userId, "Test");
        var detail = await sut.GetListAsync(created.Id, userId, isAdmin: false);

        Assert.NotNull(detail);
        Assert.NotNull(detail!.Printer);
        Assert.Equal(printerId, detail.Printer!.Id);
    }

    [Fact]
    public async Task UpdatePrinterAsync_ChangesSelectedPrinter()
    {
        var factory = CreateFactory(nameof(UpdatePrinterAsync_ChangesSelectedPrinter));
        var userId = Guid.NewGuid();
        var firstPrinterId = Guid.NewGuid();
        var secondPrinterId = Guid.NewGuid();

        await using (var db = factory.CreateDbContext())
        {
            db.Users.Add(new User { Id = userId, Username = "jamie", IsAdmin = false });
            db.PrinterConfigs.AddRange(
                new PrinterConfig
                {
                    Id = firstPrinterId,
                    Name = "Uniformation GK2",
                    BedWidthMm = 228,
                    BedDepthMm = 128,
                    IsBuiltIn = true,
                    IsDefault = true,
                },
                new PrinterConfig
                {
                    Id = secondPrinterId,
                    Name = "Saturn 4 Ultra",
                    BedWidthMm = 218,
                    BedDepthMm = 123,
                    IsBuiltIn = false,
                    IsDefault = false,
                });
            await db.SaveChangesAsync();
        }

        var sut = new PrintingListService(factory);
        var created = await sut.CreateListAsync(userId, "Test");

        var updated = await sut.UpdatePrinterAsync(created.Id, userId, isAdmin: false, secondPrinterId);

        Assert.NotNull(updated);
        Assert.NotNull(updated!.Printer);
        Assert.Equal(secondPrinterId, updated.Printer!.Id);
    }
}
