namespace findamodel.Data.Entities;

public class User
{
    public Guid Id { get; set; }
    public string Username { get; set; } = "";
    public bool IsAdmin { get; set; }
}
