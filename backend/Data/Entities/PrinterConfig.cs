namespace findamodel.Data.Entities;

public class PrinterConfig
{
    public Guid Id { get; set; }
    public string Name { get; set; } = "";
    public float BedWidthMm { get; set; }
    public float BedDepthMm { get; set; }
    public int PixelWidth { get; set; }
    public int PixelHeight { get; set; }
    public bool IsBuiltIn { get; set; }
    public bool IsDefault { get; set; }

    // Navigation: printing lists using this printer
    public List<PrintingList> PrintingLists { get; set; } = [];
}
