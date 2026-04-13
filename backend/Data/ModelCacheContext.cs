using Microsoft.EntityFrameworkCore;
using findamodel.Data.Entities;

namespace findamodel.Data;

public class ModelCacheContext(DbContextOptions<ModelCacheContext> options) : DbContext(options)
{
    public DbSet<CachedModel> Models { get; set; }
    public DbSet<DirectoryConfig> DirectoryConfigs { get; set; }
    public DbSet<AppConfig> AppConfigs { get; set; }
    public DbSet<User> Users { get; set; }
    public DbSet<PrintingList> PrintingLists { get; set; }
    public DbSet<PrintingListItem> PrintingListItems { get; set; }
    public DbSet<MetadataDictionaryValue> MetadataDictionaryValues { get; set; }
    public DbSet<IndexRun> IndexRuns { get; set; }
    public DbSet<IndexRunFile> IndexRunFiles { get; set; }
    public DbSet<IndexRunEvent> IndexRunEvents { get; set; }

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        modelBuilder.Entity<CachedModel>()
            .HasIndex(m => m.Checksum);

        modelBuilder.Entity<CachedModel>()
            .HasIndex(m => new { m.Directory, m.FileName })
            .IsUnique();

        modelBuilder.Entity<CachedModel>()
            .HasIndex(m => new { m.CalculatedCreator, m.CalculatedCollection, m.CalculatedSubcollection, m.CalculatedModelName });

        modelBuilder.Entity<CachedModel>()
            .HasIndex(m => m.CalculatedTagsJson);

        // CachedModel → DirectoryConfig (SetNull so removing a config record doesn't cascade-delete models)
        modelBuilder.Entity<CachedModel>()
            .HasOne(m => m.DirectoryConfig)
            .WithMany(d => d.Models)
            .HasForeignKey(m => m.DirectoryConfigId)
            .OnDelete(DeleteBehavior.SetNull);

        modelBuilder.Entity<DirectoryConfig>()
            .HasIndex(d => d.DirectoryPath)
            .IsUnique();

        modelBuilder.Entity<AppConfig>()
            .Property(c => c.DefaultRaftHeightMm)
            .HasDefaultValue(2f);

        modelBuilder.Entity<AppConfig>()
            .Property(c => c.TagGenerationProvider)
            .HasDefaultValue("internal");

        modelBuilder.Entity<AppConfig>()
            .Property(c => c.TagGenerationEnabled)
            .HasDefaultValue(false);

        modelBuilder.Entity<AppConfig>()
            .Property(c => c.AiDescriptionEnabled)
            .HasDefaultValue(false);

        modelBuilder.Entity<AppConfig>()
            .Property(c => c.TagGenerationEndpoint)
            .HasDefaultValue("http://localhost:11434");

        modelBuilder.Entity<AppConfig>()
            .Property(c => c.TagGenerationModel)
            .HasDefaultValue("qwen2.5vl:7b");

        modelBuilder.Entity<AppConfig>()
            .Property(c => c.TagGenerationTimeoutMs)
            .HasDefaultValue(60000);

        modelBuilder.Entity<AppConfig>()
            .Property(c => c.TagGenerationMaxTags)
            .HasDefaultValue(12);

        modelBuilder.Entity<AppConfig>()
            .Property(c => c.TagGenerationMinConfidence)
            .HasDefaultValue(0.45f);

        modelBuilder.Entity<User>()
            .HasIndex(u => u.Username)
            .IsUnique();

        modelBuilder.Entity<MetadataDictionaryValue>()
            .HasIndex(v => new { v.Field, v.NormalizedValue })
            .IsUnique();

        modelBuilder.Entity<MetadataDictionaryValue>()
            .HasIndex(v => new { v.Field, v.Value });

        modelBuilder.Entity<PrintingList>()
            .HasOne(l => l.Owner)
            .WithMany()
            .HasForeignKey(l => l.OwnerId)
            .OnDelete(DeleteBehavior.Cascade);

        modelBuilder.Entity<PrintingList>()
            .Property(l => l.SpawnType)
            .HasDefaultValue(PrintingList.DefaultSpawnType);

        modelBuilder.Entity<PrintingList>()
            .Property(l => l.HullMode)
            .HasDefaultValue(PrintingList.DefaultHullMode);

        modelBuilder.Entity<PrintingListItem>()
            .HasOne(i => i.PrintingList)
            .WithMany(l => l.Items)
            .HasForeignKey(i => i.PrintingListId)
            .OnDelete(DeleteBehavior.Cascade);

        modelBuilder.Entity<IndexRun>()
            .HasIndex(r => r.RequestedAt);

        modelBuilder.Entity<IndexRun>()
            .HasIndex(r => r.Status);

        modelBuilder.Entity<IndexRunFile>()
            .HasIndex(f => new { f.IndexRunId, f.RelativePath })
            .IsUnique();

        modelBuilder.Entity<IndexRunFile>()
            .HasOne(f => f.IndexRun)
            .WithMany(r => r.Files)
            .HasForeignKey(f => f.IndexRunId)
            .OnDelete(DeleteBehavior.Cascade);

        modelBuilder.Entity<IndexRunEvent>()
            .HasIndex(e => new { e.IndexRunId, e.CreatedAt });

        modelBuilder.Entity<IndexRunEvent>()
            .HasOne(e => e.IndexRun)
            .WithMany(r => r.Events)
            .HasForeignKey(e => e.IndexRunId)
            .OnDelete(DeleteBehavior.Cascade);

        // Self-referential: DirectoryConfig → Parent (Restrict to prevent cascade-deleting ancestor records)
        modelBuilder.Entity<DirectoryConfig>()
            .HasOne(d => d.Parent)
            .WithMany(d => d.Children)
            .HasForeignKey(d => d.ParentId)
            .OnDelete(DeleteBehavior.Restrict);
    }
}
