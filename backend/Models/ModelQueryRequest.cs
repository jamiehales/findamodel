namespace findamodel.Models;

public class ModelQueryRequest
{
    public string? Search { get; set; }
    public string[]? Creator { get; set; }
    public string[]? Collection { get; set; }
    public string[]? Subcollection { get; set; }
    public string[]? Category { get; set; }
    public string[]? Type { get; set; }
    public string[]? FileType { get; set; }
    public bool? Supported { get; set; }
    public int Limit { get; set; } = 25;
    public int Offset { get; set; } = 0;
}
