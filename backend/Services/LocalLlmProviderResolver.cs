namespace findamodel.Services;

public class LocalLlmProviderResolver(IEnumerable<ILocalLlmProvider> providers)
{
    private readonly Dictionary<string, ILocalLlmProvider> _providers = providers
        .ToDictionary(p => p.Name, p => p, StringComparer.OrdinalIgnoreCase);

    public ILocalLlmProvider Resolve(string? provider)
    {
        var requested = string.IsNullOrWhiteSpace(provider) ? "internal" : provider.Trim();
        if (_providers.TryGetValue(requested, out var resolved))
            return resolved;

        throw new InvalidOperationException($"Unsupported local LLM provider '{requested}'.");
    }
}
