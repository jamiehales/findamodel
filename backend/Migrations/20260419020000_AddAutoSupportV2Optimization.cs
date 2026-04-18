using findamodel.Data;
using Microsoft.EntityFrameworkCore.Infrastructure;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    [DbContext(typeof(ModelCacheContext))]
    [Migration("20260419020000_AddAutoSupportV2Optimization")]
    public partial class AddAutoSupportV2Optimization : Migration
    {
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<bool>(
                name: "AutoSupportV2OptimizationEnabled",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: true);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportV2CoarseVoxelSizeMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 4f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportV2FineVoxelSizeMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0.5f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportV2RefinementMarginMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 2.0f);

            migrationBuilder.AddColumn<int>(
                name: "AutoSupportV2RefinementMaxRegions",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: 12);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportV2RiskForceMarginRatio",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0.2f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportV2MinRegionVolumeMm3",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 8.0f);
        }

        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "AutoSupportV2OptimizationEnabled",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportV2CoarseVoxelSizeMm",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportV2FineVoxelSizeMm",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportV2RefinementMarginMm",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportV2RefinementMaxRegions",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportV2RiskForceMarginRatio",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportV2MinRegionVolumeMm3",
                table: "AppConfigs");
        }
    }
}
