using Serilog.Core;
using Serilog.Events;

namespace findamodel.Services;

public sealed record BufferedLogEvent(
    DateTimeOffset Timestamp,
    string Severity,
    string Channel,
    string Message,
    string? Exception);

public sealed class ApplicationLogBuffer(int maxEntries = 2000)
{
    private readonly object gate = new();
    private readonly Queue<BufferedLogEvent> events = new();
    private readonly int capacity = Math.Max(100, maxEntries);

    public void Add(LogEvent logEvent)
    {
        var channel = "General";
        if (
            logEvent.Properties.TryGetValue("SourceContext", out var sourceContext)
            && sourceContext is ScalarValue { Value: string source }
            && !string.IsNullOrWhiteSpace(source)
        )
        {
            channel = source;
        }

        var entry = new BufferedLogEvent(
            Timestamp: logEvent.Timestamp,
            Severity: logEvent.Level.ToString(),
            Channel: channel,
            Message: logEvent.RenderMessage(),
            Exception: logEvent.Exception?.ToString());

        lock (gate)
        {
            events.Enqueue(entry);
            while (events.Count > capacity)
            {
                events.Dequeue();
            }
        }
    }

    public IReadOnlyList<BufferedLogEvent> Get(string? channel, LogEventLevel? minimumLevel, int limit)
    {
        var normalizedChannel = string.IsNullOrWhiteSpace(channel) ? null : channel.Trim();
        var boundedLimit = Math.Clamp(limit, 1, 1000);
        List<BufferedLogEvent> snapshot;
        lock (gate)
        {
            snapshot = [.. events];
        }

        IEnumerable<BufferedLogEvent> query = snapshot;
        if (normalizedChannel != null)
        {
            query = query.Where(e =>
                string.Equals(e.Channel, normalizedChannel, StringComparison.OrdinalIgnoreCase));
        }

        if (minimumLevel is not null)
        {
            query = query.Where(e =>
                Enum.TryParse<LogEventLevel>(e.Severity, ignoreCase: true, out var level)
                && level >= minimumLevel.Value);
        }

        return [.. query.Reverse().Take(boundedLimit)];
    }

    public IReadOnlyList<string> GetAvailableChannels()
    {
        lock (gate)
        {
            return
            [
                .. events
                    .Select(e => e.Channel)
                    .Where(c => !string.IsNullOrWhiteSpace(c))
                    .Distinct(StringComparer.OrdinalIgnoreCase)
                    .OrderBy(c => c, StringComparer.OrdinalIgnoreCase),
            ];
        }
    }
}

public sealed class ApplicationLogSink(ApplicationLogBuffer buffer) : ILogEventSink
{
    public void Emit(LogEvent logEvent)
    {
        buffer.Add(logEvent);
    }
}