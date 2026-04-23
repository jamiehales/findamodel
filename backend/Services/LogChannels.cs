namespace findamodel.Services;

/// <summary>
/// Named logging channels. Each channel maps to a Serilog category prefix that can be
/// independently enabled or disabled via <c>Serilog:MinimumLevel:Override</c> in appsettings.
/// </summary>
public static class LogChannels
{
    private const string Root = "findamodel";

    /// <summary>Directory config file reading and writing.</summary>
    public const string Config = $"{Root}.Config";

    /// <summary>Hull geometry calculation.</summary>
    public const string Hull = $"{Root}.Hull";

    /// <summary>Model indexing and queue orchestration.</summary>
    public const string Indexing = $"{Root}.Indexing";

    /// <summary>STL/OBJ geometry loading.</summary>
    public const string Loader = $"{Root}.Loader";

    /// <summary>Model metadata, queries, and CRUD.</summary>
    public const string Models = $"{Root}.Models";

    /// <summary>Preview image generation.</summary>
    public const string Preview = $"{Root}.Preview";

    /// <summary>Printing list management and archive jobs.</summary>
    public const string PrintingList = $"{Root}.PrintingList";

    /// <summary>Explorer browsing and directory listing.</summary>
    public const string Explorer = $"{Root}.Explorer";

    /// <summary>Query execution and search.</summary>
    public const string Query = $"{Root}.Query";

    /// <summary>Metadata config and dictionary services.</summary>
    public const string Metadata = $"{Root}.Metadata";

    /// <summary>Application config persistence.</summary>
    public const string AppConfig = $"{Root}.AppConfig";

    /// <summary>Local LLM provider and inference orchestration.</summary>
    public const string Llm = $"{Root}.Llm";

    /// <summary>Pre-slice mesh repair pipeline.</summary>
    public const string Repair = $"{Root}.Repair";
}
