using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddAutoSupportConfigSettings : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<float>(
                name: "AutoSupportBedMarginMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportMaxLayerHeightMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0f);

            migrationBuilder.AddColumn<int>(
                name: "AutoSupportMaxSupportsPerIsland",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: 0);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportMaxVoxelSizeMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportMergeDistanceMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportMinLayerHeightMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportMinVoxelSizeMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportPullForceThreshold",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportSphereRadiusMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0f);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "AutoSupportBedMarginMm",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportMaxLayerHeightMm",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportMaxSupportsPerIsland",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportMaxVoxelSizeMm",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportMergeDistanceMm",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportMinLayerHeightMm",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportMinVoxelSizeMm",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportPullForceThreshold",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportSphereRadiusMm",
                table: "AppConfigs");
        }
    }
}
