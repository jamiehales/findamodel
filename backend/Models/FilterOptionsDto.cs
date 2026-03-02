namespace findamodel.Models;

public class FilterOptionsDto
{
    public List<string> Creators { get; set; } = [];
    public List<string> Collections { get; set; } = [];
    public List<string> Subcollections { get; set; } = [];
    public List<string> Categories { get; set; } = [];
    public List<string> Types { get; set; } = [];
    public List<string> FileTypes { get; set; } = [];
}
