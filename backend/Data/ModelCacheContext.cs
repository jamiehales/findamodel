using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Storage.ValueConversion;
using findamodel.Data.Entities;

namespace findamodel.Data;

public class ModelCacheContext(DbContextOptions<ModelCacheContext> options) : DbContext(options)
{
    private static readonly ValueConverter<DateTime, DateTime> UtcDateTimeConverter = new(
        value => value.Kind == DateTimeKind.Utc ? value : value.ToUniversalTime(),
        value => DateTime.SpecifyKind(value, DateTimeKind.Utc));

    private static readonly ValueConverter<DateTime?, DateTime?> NullableUtcDateTimeConverter = new(
        value => value.HasValue
            ? (value.Value.Kind == DateTimeKind.Utc ? value : value.Value.ToUniversalTime())
            : value,
        value => value.HasValue
            ? DateTime.SpecifyKind(value.Value, DateTimeKind.Utc)
            : value);

    public DbSet<CachedModel> Models { get; set; }
    public DbSet<DirectoryConfig> DirectoryConfigs { get; set; }
    public DbSet<AppConfig> AppConfigs { get; set; }
    public DbSet<User> Users { get; set; }
    public DbSet<PrintingList> PrintingLists { get; set; }
    public DbSet<PrintingListItem> PrintingListItems { get; set; }
    public DbSet<PrinterConfig> PrinterConfigs { get; set; }
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

        modelBuilder.Entity<PrinterConfig>()
            .HasIndex(p => p.IsDefault);

        modelBuilder.Entity<PrintingList>()
            .HasOne(l => l.PrinterConfig)
            .WithMany(p => p.PrintingLists)
            .HasForeignKey(l => l.PrinterConfigId)
            .OnDelete(DeleteBehavior.SetNull);

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

        foreach (var entityType in modelBuilder.Model.GetEntityTypes())
        {
            foreach (var property in entityType.GetProperties())
            {
                if (property.ClrType == typeof(DateTime))
                    property.SetValueConverter(UtcDateTimeConverter);
                else if (property.ClrType == typeof(DateTime?))
                    property.SetValueConverter(NullableUtcDateTimeConverter);
            }
        }
    }
}
