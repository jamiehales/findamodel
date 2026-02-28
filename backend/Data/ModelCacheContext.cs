using Microsoft.EntityFrameworkCore;
using findamodel.Data.Entities;

namespace findamodel.Data;

public class ModelCacheContext(DbContextOptions<ModelCacheContext> options) : DbContext(options)
{
    public DbSet<CachedModel> Models { get; set; }

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        modelBuilder.Entity<CachedModel>()
            .HasIndex(m => m.Checksum)
            .IsUnique();
    }
}
