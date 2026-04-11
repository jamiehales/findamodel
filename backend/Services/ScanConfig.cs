using System.Globalization;

namespace findamodel.Services;

/// <summary>
/// Canonical source of truth for the scan configuration checksum.
/// This checksum encodes every input that can affect hull/scan generation.
/// IMPORTANT (AI agents): if any new configuration input influences hull generation
/// (e.g. a new algorithm version constant, a mesh tolerance setting, a new raft parameter),
/// add it to <see cref="Compute"/> below so that existing cached hulls are automatically
/// invalidated on the next scan. Do not add new staleness checks elsewhere.
/// </summary>
public static class ScanConfig
{
    /// <summary>
    /// Returns a canonical string that uniquely identifies the set of configuration inputs
    /// used to produce a hull for a given model. Stored on <c>CachedModel.ScanConfigChecksum</c>.
    /// Inequality between the stored value and the current computed value means the hull is stale.
    /// </summary>
    public static string Compute(float raftHeightMm) =>
        $"hull:{HullCalculationService.CurrentHullGenerationVersion}|raft:{raftHeightMm.ToString("F4", CultureInfo.InvariantCulture)}";
}
