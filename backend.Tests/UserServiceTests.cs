using findamodel.Data;
using findamodel.Data.Entities;
using findamodel.Services;
using Microsoft.EntityFrameworkCore;
using Xunit;

namespace findamodel.Tests;

public class UserServiceTests
{
    // ── Helpers ───────────────────────────────────────────────────────────────

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

    // ── SeedAdminUserAsync ────────────────────────────────────────────────────

    [Fact]
    public async Task SeedAdminUserAsync_CreatesAdminUser_WhenDbIsEmpty()
    {
        var factory = CreateFactory(nameof(SeedAdminUserAsync_CreatesAdminUser_WhenDbIsEmpty));
        var sut = new UserService(factory);
        await sut.SeedAdminUserAsync();

        await using var db = factory.CreateDbContext();
        var user = await db.Users.SingleAsync();
        Assert.Equal("admin", user.Username);
        Assert.True(user.IsAdmin);
    }

    [Fact]
    public async Task SeedAdminUserAsync_DoesNotAddDuplicates_WhenUsersAlreadyExist()
    {
        var factory = CreateFactory(nameof(SeedAdminUserAsync_DoesNotAddDuplicates_WhenUsersAlreadyExist));
        await using (var seed = factory.CreateDbContext())
        {
            seed.Users.Add(new User { Id = Guid.NewGuid(), Username = "admin", IsAdmin = true });
            await seed.SaveChangesAsync();
        }

        var sut = new UserService(factory);
        await sut.SeedAdminUserAsync();   // called again

        await using var db = factory.CreateDbContext();
        Assert.Equal(1, await db.Users.CountAsync());
    }

    [Fact]
    public async Task SeedAdminUserAsync_IsIdempotent_MultipleCallsOnEmptyDb()
    {
        var factory = CreateFactory(nameof(SeedAdminUserAsync_IsIdempotent_MultipleCallsOnEmptyDb));
        var sut = new UserService(factory);
        await sut.SeedAdminUserAsync();
        await sut.SeedAdminUserAsync();

        await using var db = factory.CreateDbContext();
        Assert.Equal(1, await db.Users.CountAsync());
    }

    // ── GetUserByIdAsync ──────────────────────────────────────────────────────

    [Fact]
    public async Task GetUserByIdAsync_ReturnsNull_WhenUserDoesNotExist()
    {
        var factory = CreateFactory(nameof(GetUserByIdAsync_ReturnsNull_WhenUserDoesNotExist));
        var sut = new UserService(factory);
        var result = await sut.GetUserByIdAsync(Guid.NewGuid());
        Assert.Null(result);
    }

    [Fact]
    public async Task GetUserByIdAsync_ReturnsUser_WhenFound()
    {
        var factory = CreateFactory(nameof(GetUserByIdAsync_ReturnsUser_WhenFound));
        var id = Guid.NewGuid();

        await using (var seed = factory.CreateDbContext())
        {
            seed.Users.Add(new User { Id = id, Username = "alice", IsAdmin = false });
            await seed.SaveChangesAsync();
        }

        var sut = new UserService(factory);
        var user = await sut.GetUserByIdAsync(id);
        Assert.NotNull(user);
        Assert.Equal("alice", user!.Username);
        Assert.Equal(id, user.Id);
    }

    // ── GetAdminUserAsync ─────────────────────────────────────────────────────

    [Fact]
    public async Task GetAdminUserAsync_ReturnsNull_WhenDbEmpty()
    {
        var factory = CreateFactory(nameof(GetAdminUserAsync_ReturnsNull_WhenDbEmpty));
        var sut = new UserService(factory);
        var result = await sut.GetAdminUserAsync();
        Assert.Null(result);
    }

    [Fact]
    public async Task GetAdminUserAsync_ReturnsAdminUser()
    {
        var factory = CreateFactory(nameof(GetAdminUserAsync_ReturnsAdminUser));
        var adminId = Guid.NewGuid();

        await using (var seed = factory.CreateDbContext())
        {
            seed.Users.Add(new User { Id = Guid.NewGuid(), Username = "alice", IsAdmin = false });
            seed.Users.Add(new User { Id = adminId, Username = "admin", IsAdmin = true });
            await seed.SaveChangesAsync();
        }

        var sut = new UserService(factory);
        var admin = await sut.GetAdminUserAsync();
        Assert.NotNull(admin);
        Assert.Equal(adminId, admin!.Id);
        Assert.True(admin.IsAdmin);
    }

    [Fact]
    public async Task GetAdminUserAsync_ReturnsNull_WhenNoAdminExists()
    {
        var factory = CreateFactory(nameof(GetAdminUserAsync_ReturnsNull_WhenNoAdminExists));

        await using (var seed = factory.CreateDbContext())
        {
            seed.Users.Add(new User { Id = Guid.NewGuid(), Username = "alice", IsAdmin = false });
            await seed.SaveChangesAsync();
        }

        var sut = new UserService(factory);
        Assert.Null(await sut.GetAdminUserAsync());
    }
}
