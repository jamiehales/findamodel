using Cronos;

namespace findamodel.Services;

public class ModelIndexerService(
    ModelService modelService,
    IConfiguration config,
    ILogger<ModelIndexerService> logger) : BackgroundService
{
    private const string DefaultSchedule = "0 3 * * *";

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        // _ = SyncMetadataSafely();

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
                logger.LogInformation("ModelIndexerService: running scheduled scan (limit: 100)");
                await RunScanSafely(limit: 100);
            }
        }

        logger.LogInformation("ModelIndexerService: stopped");
    }

    private async Task RunScanSafely(int? limit = null)
    {
        try
        {
            await modelService.ScanAndCacheAsync(limit: limit);
        }
        catch (Exception ex)
        {
            logger.LogError(ex, "ModelIndexerService: scan failed");
        }
    }

    private async Task SyncMetadataSafely()
    {
        try
        {
            await modelService.SyncDirectoryConfigsAsync();
        }
        catch (Exception ex)
        {
            logger.LogError(ex, "ModelIndexerService: directory config sync failed");
        }
    }
}
