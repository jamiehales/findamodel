using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class BackfillAutoSupportConfigValues : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.Sql(@"
UPDATE AppConfigs
SET AutoSupportBedMarginMm = CASE WHEN AutoSupportBedMarginMm = 0 THEN 2.0 ELSE AutoSupportBedMarginMm END,
    AutoSupportMinVoxelSizeMm = CASE WHEN AutoSupportMinVoxelSizeMm = 0 THEN 0.8 ELSE AutoSupportMinVoxelSizeMm END,
    AutoSupportMaxVoxelSizeMm = CASE WHEN AutoSupportMaxVoxelSizeMm = 0 THEN 2.0 ELSE AutoSupportMaxVoxelSizeMm END,
    AutoSupportMinLayerHeightMm = CASE WHEN AutoSupportMinLayerHeightMm = 0 THEN 0.75 ELSE AutoSupportMinLayerHeightMm END,
    AutoSupportMaxLayerHeightMm = CASE WHEN AutoSupportMaxLayerHeightMm = 0 THEN 1.5 ELSE AutoSupportMaxLayerHeightMm END,
    AutoSupportMergeDistanceMm = CASE WHEN AutoSupportMergeDistanceMm = 0 THEN 2.5 ELSE AutoSupportMergeDistanceMm END,
    AutoSupportPullForceThreshold = CASE WHEN AutoSupportPullForceThreshold = 0 THEN 3.0 ELSE AutoSupportPullForceThreshold END,
    AutoSupportSphereRadiusMm = CASE WHEN AutoSupportSphereRadiusMm = 0 THEN 1.2 ELSE AutoSupportSphereRadiusMm END,
    AutoSupportMaxSupportsPerIsland = CASE WHEN AutoSupportMaxSupportsPerIsland = 0 THEN 6 ELSE AutoSupportMaxSupportsPerIsland END
WHERE Id = 1;");
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
        }
    }
}
