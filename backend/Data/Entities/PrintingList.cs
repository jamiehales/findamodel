namespace findamodel.Data.Entities;

public class PrintingList
{
    public const string DefaultSpawnType = "grouped";
    public const string DefaultHullMode = "convex";

    public Guid Id { get; set; }
    public string Name { get; set; } = "";
    public Guid OwnerId { get; set; }
    public User Owner { get; set; } = null!;
    public bool IsActive { get; set; }
    public bool IsDefault { get; set; }
    public string SpawnType { get; set; } = DefaultSpawnType;
    public string HullMode { get; set; } = DefaultHullMode;
    public DateTime CreatedAt { get; set; }
    public List<PrintingListItem> Items { get; set; } = [];
}
