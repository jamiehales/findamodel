namespace findamodel.Data.Entities;

public class PrintingList
{
    public Guid Id { get; set; }
    public string Name { get; set; } = "";
    public Guid OwnerId { get; set; }
    public User Owner { get; set; } = null!;
    public bool IsActive { get; set; }
    public bool IsDefault { get; set; }
    public DateTime CreatedAt { get; set; }
    public List<PrintingListItem> Items { get; set; } = [];
}
