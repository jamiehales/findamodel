using Microsoft.Data.Sqlite;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Logging;
using findamodel.Data;
using findamodel.Models;
using findamodel.Services;
using Xunit;

namespace findamodel.Tests;

public class IndexerServiceTests
{
    private sealed class SqliteDbContextFactory(DbContextOptions<ModelCacheContext> options)
        : IDbContextFactory<ModelCacheContext>
    {
        public ModelCacheContext CreateDbContext() => new(options);

        public Task<ModelCacheContext> CreateDbContextAsync(CancellationToken ct = default) =>
            Task.FromResult(CreateDbContext());
    }

    private sealed class BlockingFirstCreateDbContextFactory(IDbContextFactory<ModelCacheContext> inner)
        : IDbContextFactory<ModelCacheContext>
    {
        private int _calls;
        private readonly TaskCompletionSource _firstCallSeen =
            new(TaskCreationOptions.RunContinuationsAsynchronously);
        private readonly TaskCompletionSource _allowFirstCallToProceed =
            new(TaskCreationOptions.RunContinuationsAsynchronously);

        public Task FirstCallSeen => _firstCallSeen.Task;

        public void ReleaseFirstCall() => _allowFirstCallToProceed.TrySetResult();

        public ModelCacheContext CreateDbContext() => inner.CreateDbContext();

        public async Task<ModelCacheContext> CreateDbContextAsync(CancellationToken ct = default)
        {
            if (Interlocked.Increment(ref _calls) == 1)
            {
                _firstCallSeen.TrySetResult();
                await _allowFirstCallToProceed.Task.WaitAsync(ct);
            }

            return await inner.CreateDbContextAsync(ct);
        }
    }

    /// <summary>
    /// Lets the first call go through immediately, then blocks all subsequent calls
    /// until <see cref="ReleaseAll"/> is called.  Used to let one-shot startup methods
    /// (e.g. RequeueInterruptedRunsAsync) complete while preventing the background
    /// processing loop from touching the DB.
    /// </summary>
    private sealed class BlockingAfterFirstCallDbContextFactory(IDbContextFactory<ModelCacheContext> inner)
        : IDbContextFactory<ModelCacheContext>
    {
        private int _calls;
        private readonly TaskCompletionSource _secondCallBlocked =
            new(TaskCreationOptions.RunContinuationsAsynchronously);
        private readonly TaskCompletionSource _allowContinue =
            new(TaskCreationOptions.RunContinuationsAsynchronously);

        public Task SecondCallBlocked => _secondCallBlocked.Task;
        public void ReleaseAll() => _allowContinue.TrySetResult();

        public ModelCacheContext CreateDbContext() => inner.CreateDbContext();

        public async Task<ModelCacheContext> CreateDbContextAsync(CancellationToken ct = default)
        {
            if (Interlocked.Increment(ref _calls) > 1)
            {
                _secondCallBlocked.TrySetResult();
                await _allowContinue.Task.WaitAsync(ct);
            }
            return await inner.CreateDbContextAsync(ct);
        }
    }

    private static (IDbContextFactory<ModelCacheContext> Factory, SqliteConnection Connection) CreateFactory()
    {
        var connection = new SqliteConnection("Data Source=:memory:");
        connection.Open();

        var options = new DbContextOptionsBuilder<ModelCacheContext>()
            .UseSqlite(connection)
            .Options;

        using var db = new ModelCacheContext(options);
        db.Database.EnsureCreated();

        return (new SqliteDbContextFactory(options), connection);
    }

    private static async Task WaitUntilAsync(Func<bool> condition, TimeSpan timeout)
    {
        var deadline = DateTime.UtcNow + timeout;
        while (DateTime.UtcNow < deadline)
        {
            if (condition())
                return;

            await Task.Delay(20);
        }

        Assert.True(condition(), "Condition was not met within timeout.");
    }

    [Fact]
    public async Task Enqueue_ProcessesInFifoOrder_WhileQueueDisplayIsNewestFirst()
    {
        var (factory, connection) = CreateFactory();
        var blockingFactory = new BlockingFirstCreateDbContextFactory(factory);
        try
        {
            using var loggerFactory = LoggerFactory.Create(_ => { });
            var sut = new IndexerService(
                metadataConfigService: null!,
                modelService: null!,
                dbFactory: blockingFactory,
                loggerFactory: loggerFactory);

            var first = sut.Enqueue("a", null, IndexFlags.None);
            var second = sut.Enqueue("b", null, IndexFlags.None);
            var third = sut.Enqueue("c", null, IndexFlags.None);

            await blockingFactory.FirstCallSeen.WaitAsync(TimeSpan.FromSeconds(2));
            await WaitUntilAsync(() => sut.GetStatus().CurrentRequest is not null, TimeSpan.FromSeconds(2));

            var blockedStatus = sut.GetStatus();
            Assert.Equal(first.Id, blockedStatus.CurrentRequest?.Id);

            // Queue presentation is newest-first for UI readability.
            Assert.Equal(2, blockedStatus.Queue.Count);
            Assert.Equal(third.Id, blockedStatus.Queue[0].Id);
            Assert.Equal(second.Id, blockedStatus.Queue[1].Id);

            blockingFactory.ReleaseFirstCall();
            await WaitUntilAsync(() => !sut.GetStatus().IsRunning, TimeSpan.FromSeconds(5));

            await using var db = await factory.CreateDbContextAsync();
            var secondRun = await db.IndexRuns.FirstOrDefaultAsync(r => r.Id == second.RunId);
            var thirdRun = await db.IndexRuns.FirstOrDefaultAsync(r => r.Id == third.RunId);

            Assert.NotNull(secondRun);
            Assert.NotNull(thirdRun);
            Assert.NotNull(secondRun!.StartedAt);
            Assert.NotNull(thirdRun!.StartedAt);

            // Actual processing order must stay FIFO.
            Assert.True(secondRun.StartedAt <= thirdRun.StartedAt);
        }
        finally
        {
            await connection.DisposeAsync();
        }
    }

    [Fact]
    public async Task GetStatus_QueueOrder_IsDescendingByRequestedTime()
    {
        var (factory, connection) = CreateFactory();
        var blockingFactory = new BlockingFirstCreateDbContextFactory(factory);
        try
        {
            using var loggerFactory = LoggerFactory.Create(_ => { });
            var sut = new IndexerService(
                metadataConfigService: null!,
                modelService: null!,
                dbFactory: blockingFactory,
                loggerFactory: loggerFactory);


            var oldest = sut.Enqueue("oldest", null, IndexFlags.None);
            await Task.Delay(10);
            var middle = sut.Enqueue("middle", null, IndexFlags.None);
            await Task.Delay(10);
            var newest = sut.Enqueue("newest", null, IndexFlags.None);

            await blockingFactory.FirstCallSeen.WaitAsync(TimeSpan.FromSeconds(2));
            await WaitUntilAsync(() => sut.GetStatus().CurrentRequest is not null, TimeSpan.FromSeconds(2));

            var status = sut.GetStatus();
            Assert.Equal(2, status.Queue.Count);
            Assert.Equal(newest.Id, status.Queue[0].Id);
            Assert.Equal(middle.Id, status.Queue[1].Id);
        }
        finally
        {
            blockingFactory.ReleaseFirstCall();
            await connection.DisposeAsync();
        }
    }

    [Fact]
    public async Task RequeueInterruptedRunsAsync_ResetsRunningRunsToQueued_NotFailed()
    {
        var (factory, connection) = CreateFactory();
        var blockingFactory = new BlockingAfterFirstCallDbContextFactory(factory);
        try
        {
            // Seed two IndexRun records left in "running" state (simulating a server crash).
            var runA = new findamodel.Data.Entities.IndexRun
            {
                Id = Guid.NewGuid(),
                DirectoryFilter = "cats",
                Flags = (int)IndexFlags.Models,
                RequestedAt = DateTime.UtcNow.AddMinutes(-5),
                StartedAt = DateTime.UtcNow.AddMinutes(-4),
                Status = "running",
                ProcessedFiles = 3,
                TotalFiles = 10,
            };
            var runB = new findamodel.Data.Entities.IndexRun
            {
                Id = Guid.NewGuid(),
                DirectoryFilter = "dogs",
                Flags = (int)IndexFlags.Models,
                RequestedAt = DateTime.UtcNow.AddMinutes(-3),
                StartedAt = DateTime.UtcNow.AddMinutes(-2),
                Status = "running",
                ProcessedFiles = 1,
                TotalFiles = 5,
            };

            await using (var db = await factory.CreateDbContextAsync())
            {
                db.IndexRuns.AddRange(runA, runB);
                await db.SaveChangesAsync();
            }

            using var loggerFactory = LoggerFactory.Create(_ => { });
            var sut = new IndexerService(
                metadataConfigService: null!,
                modelService: null!,
                    dbFactory: blockingFactory,
                loggerFactory: loggerFactory);

            await sut.RequeueInterruptedRunsAsync();

            // Wait until the background processing task has started (its first DB call
            // is blocked by blockingFactory), then assert the intermediate state.
            // runA (oldest) has been dequeued to CurrentRequest; runB is still in the queue.
            // CreateRunAsync has NOT been called yet, so DB records are still "queued".
            await blockingFactory.SecondCallBlocked.WaitAsync(TimeSpan.FromSeconds(2));

            // DB records must still be "queued" — CreateRunAsync hasn't run yet.
            await using (var db = await factory.CreateDbContextAsync())
            {
                var a = await db.IndexRuns.FindAsync(runA.Id);
                var b = await db.IndexRuns.FindAsync(runB.Id);

                Assert.NotNull(a);
                Assert.Equal("queued", a!.Status);
                Assert.Null(a.Outcome);
                Assert.Null(a.Error);
                Assert.Null(a.StartedAt);
                Assert.Equal(0, a.ProcessedFiles);

                Assert.NotNull(b);
                Assert.Equal("queued", b!.Status);
            }

            // runA (oldest RequestedAt) must be the active request; runB must be in queue.
            // Both original RunIds must be preserved.
            var status = sut.GetStatus();
            Assert.Equal(runA.Id, status.CurrentRequest?.RunId);
            Assert.Single(status.Queue);
            Assert.Equal(runB.Id, status.Queue[0].RunId);
        }
        finally
        {
            blockingFactory.ReleaseAll();
            await connection.DisposeAsync();
        }
    }

    [Fact]
    public async Task CancelAsync_RemovesQueuedRequest_AndMarksRunCancelled()
    {
        var (factory, connection) = CreateFactory();
        var blockingFactory = new BlockingFirstCreateDbContextFactory(factory);
        try
        {
            using var loggerFactory = LoggerFactory.Create(_ => { });
            var sut = new IndexerService(
                metadataConfigService: null!,
                modelService: null!,
                dbFactory: blockingFactory,
                loggerFactory: loggerFactory);

            var first = sut.Enqueue("alpha", null, IndexFlags.None);
            var second = sut.Enqueue("beta", null, IndexFlags.None);

            await blockingFactory.FirstCallSeen.WaitAsync(TimeSpan.FromSeconds(2));
            await WaitUntilAsync(() => sut.GetStatus().CurrentRequest is not null, TimeSpan.FromSeconds(2));

            var cancelled = await sut.CancelAsync(second.RunId!.Value);

            Assert.True(cancelled);
            var status = sut.GetStatus();
            Assert.DoesNotContain(status.Queue, q => q.RunId == second.RunId);
            Assert.True(status.CurrentRequest is null || status.CurrentRequest.RunId != second.RunId);

            await using var db = await factory.CreateDbContextAsync();
            var cancelledRun = await db.IndexRuns.FirstOrDefaultAsync(r => r.Id == second.RunId);
            Assert.NotNull(cancelledRun);
            Assert.Equal("cancelled", cancelledRun!.Status);
            Assert.Equal("cancelled", cancelledRun.Outcome);

            // Ensure the remaining request can complete so background work exits cleanly.
            blockingFactory.ReleaseFirstCall();
            await WaitUntilAsync(() => !sut.GetStatus().IsRunning, TimeSpan.FromSeconds(5));
            Assert.Equal(first.RunId, sut.GetStatus().Recent.FirstOrDefault()?.RunId);
        }
        finally
        {
            blockingFactory.ReleaseFirstCall();
            await connection.DisposeAsync();
        }
    }

    [Fact]
    public async Task CancelAsync_CancelsRunningRequest_AndPersistsCancelledOutcome()
    {
        var (factory, connection) = CreateFactory();
        var blockingFactory = new BlockingFirstCreateDbContextFactory(factory);
        try
        {
            using var loggerFactory = LoggerFactory.Create(_ => { });
            var sut = new IndexerService(
                metadataConfigService: null!,
                modelService: null!,
                dbFactory: blockingFactory,
                loggerFactory: loggerFactory);

            var request = sut.Enqueue("running", null, IndexFlags.None);

            await blockingFactory.FirstCallSeen.WaitAsync(TimeSpan.FromSeconds(2));
            await WaitUntilAsync(() => sut.GetStatus().CurrentRequest is not null, TimeSpan.FromSeconds(2));

            var cancelled = await sut.CancelAsync(request.RunId!.Value);
            Assert.True(cancelled);

            // Allow the blocked DB operation to continue so cancellation can be persisted.
            blockingFactory.ReleaseFirstCall();
            await WaitUntilAsync(() => !sut.GetStatus().IsRunning, TimeSpan.FromSeconds(5));

            var recent = sut.GetStatus().Recent.FirstOrDefault();
            Assert.NotNull(recent);
            Assert.Equal(request.RunId, recent!.RunId);
            Assert.Equal("cancelled", recent.Outcome);

            await using var db = await factory.CreateDbContextAsync();
            var run = await db.IndexRuns.FirstOrDefaultAsync(r => r.Id == request.RunId);
            Assert.NotNull(run);
            Assert.Equal("cancelled", run!.Status);
            Assert.Equal("cancelled", run.Outcome);
        }
        finally
        {
            blockingFactory.ReleaseFirstCall();
            await connection.DisposeAsync();
        }
    }
}
