namespace findamodel.Data.Entities;

public class AppConfig
{
    public int Id { get; set; }
    public float DefaultRaftHeightMm { get; set; } = 2f;
    public string Theme { get; set; } = "nord";
    public DateTime UpdatedAt { get; set; }
}
