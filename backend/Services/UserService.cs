using findamodel.Data;
using findamodel.Data.Entities;
using Microsoft.EntityFrameworkCore;

namespace findamodel.Services;

public class UserService(IDbContextFactory<ModelCacheContext> dbFactory)
{
    public async Task SeedAdminUserAsync()
    {
        using var db = dbFactory.CreateDbContext();
        if (!await db.Users.AnyAsync())
        {
            db.Users.Add(new User
            {
                Id = Guid.NewGuid(),
                Username = "admin",
                IsAdmin = true,
            });
            await db.SaveChangesAsync();
        }
    }

    public async Task<User?> GetUserByIdAsync(Guid id)
    {
        using var db = dbFactory.CreateDbContext();
        return await db.Users.FindAsync(id);
    }

    public async Task<User?> GetAdminUserAsync()
    {
        using var db = dbFactory.CreateDbContext();
        return await db.Users.FirstOrDefaultAsync(u => u.IsAdmin);
    }
}
