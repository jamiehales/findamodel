using Microsoft.EntityFrameworkCore;
using findamodel.Data.Entities;

namespace findamodel.Data;

public class ModelCacheContext(DbContextOptions<ModelCacheContext> options) : DbContext(options)
{
    public DbSet<CachedModel> Models { get; set; }
    public DbSet<DirectoryConfig> DirectoryConfigs { get; set; }
    public DbSet<User> Users { get; set; }
    public DbSet<PrintingList> PrintingLists { get; set; }
    public DbSet<PrintingListItem> PrintingListItems { get; set; }

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        modelBuilder.Entity<CachedModel>()
            .HasIndex(m => m.Checksum);

        // CachedModel → DirectoryConfig (SetNull so removing a config record doesn't cascade-delete models)
        modelBuilder.Entity<CachedModel>()
            .HasOne(m => m.DirectoryConfig)
            .WithMany(d => d.Models)
            .HasForeignKey(m => m.DirectoryConfigId)
            .OnDelete(DeleteBehavior.SetNull);

        modelBuilder.Entity<DirectoryConfig>()
            .HasIndex(d => d.DirectoryPath)
            .IsUnique();

        modelBuilder.Entity<User>()
            .HasIndex(u => u.Username)
            .IsUnique();

        modelBuilder.Entity<PrintingList>()
            .HasOne(l => l.Owner)
            .WithMany()
            .HasForeignKey(l => l.OwnerId)
            .OnDelete(DeleteBehavior.Cascade);

        modelBuilder.Entity<PrintingListItem>()
            .HasOne(i => i.PrintingList)
            .WithMany(l => l.Items)
            .HasForeignKey(i => i.PrintingListId)
            .OnDelete(DeleteBehavior.Cascade);

        // Self-referential: DirectoryConfig → Parent (Restrict to prevent cascade-deleting ancestor records)
        modelBuilder.Entity<DirectoryConfig>()
            .HasOne(d => d.Parent)
            .WithMany(d => d.Children)
            .HasForeignKey(d => d.ParentId)
            .OnDelete(DeleteBehavior.Restrict);
    }
}
