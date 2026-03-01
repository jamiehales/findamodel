namespace findamodel.Data.Entities;

public class PrintingListItem
{
    public Guid Id { get; set; }
    public Guid PrintingListId { get; set; }
    public PrintingList PrintingList { get; set; } = null!;
    public Guid ModelId { get; set; }
    public int Quantity { get; set; }
}
