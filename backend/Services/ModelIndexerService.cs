using Cronos;
using findamodel.Models;

namespace findamodel.Services;

public class ModelIndexerService(
    IndexerService indexerService,
    IConfiguration config,
    ILoggerFactory loggerFactory) : BackgroundService
{
    private readonly ILogger logger = loggerFactory.CreateLogger(LogChannels.Indexing);
    private const string DefaultSchedule = "0 3 * * *";

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        if (config.GetValue("Indexing:IndexOnStartup", true))
            indexerService.Enqueue(null, null, IndexFlags.Directories);

        var expression = config["Models:IndexSchedule"] ?? DefaultSchedule;
        CronExpression cron;
        try
        {
            cron = CronExpression.Parse(expression);
        }
        catch (CronFormatException ex)
        {
            logger.LogError(ex, "Invalid cron expression '{Expression}'. Indexer will not run again.", expression);
            return;
        }

        while (!stoppingToken.IsCancellationRequested)
        {
            var next = cron.GetNextOccurrence(DateTime.UtcNow, TimeZoneInfo.Utc);
            if (next is null)
            {
                logger.LogWarning("Cron expression '{Expression}' has no future occurrences. Indexer stopped.", expression);
                return;
            }

            logger.LogInformation("ModelIndexerService: next scan scheduled at {Next} UTC", next.Value);
            try
            {
                await Task.Delay(next.Value - DateTime.UtcNow, stoppingToken);
            }
            catch (OperationCanceledException)
            {
                break;
            }

            if (!stoppingToken.IsCancellationRequested)
            {
                logger.LogInformation("ModelIndexerService: enqueueing scheduled full scan");
                indexerService.Enqueue(null, null, IndexFlags.Directories | IndexFlags.Models);
            }
        }

        logger.LogInformation("ModelIndexerService: stopped");
    }
}
