namespace findamodel.Data.Entities;

public class MetadataDictionaryValue
{
    public Guid Id { get; set; }
    public string Field { get; set; } = "";
    public string Value { get; set; } = "";
    public string NormalizedValue { get; set; } = "";
    public DateTime CreatedAt { get; set; }
    public DateTime UpdatedAt { get; set; }
}
