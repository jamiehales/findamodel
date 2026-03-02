namespace findamodel.Models;

public class ModelQueryResult
{
    public List<ModelDto> Models { get; set; } = [];
    public int TotalCount { get; set; }
    public bool HasMore { get; set; }
}
