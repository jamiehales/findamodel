using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class SetAutoSupportConfigDefaults : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AlterColumn<float>(
                name: "AutoSupportSphereRadiusMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 1.2f,
                oldClrType: typeof(float),
                oldType: "REAL");

            migrationBuilder.AlterColumn<float>(
                name: "AutoSupportPullForceThreshold",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 3f,
                oldClrType: typeof(float),
                oldType: "REAL");

            migrationBuilder.AlterColumn<float>(
                name: "AutoSupportMinVoxelSizeMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0.8f,
                oldClrType: typeof(float),
                oldType: "REAL");

            migrationBuilder.AlterColumn<float>(
                name: "AutoSupportMinLayerHeightMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0.75f,
                oldClrType: typeof(float),
                oldType: "REAL");

            migrationBuilder.AlterColumn<float>(
                name: "AutoSupportMergeDistanceMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 2.5f,
                oldClrType: typeof(float),
                oldType: "REAL");

            migrationBuilder.AlterColumn<float>(
                name: "AutoSupportMaxVoxelSizeMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 2f,
                oldClrType: typeof(float),
                oldType: "REAL");

            migrationBuilder.AlterColumn<int>(
                name: "AutoSupportMaxSupportsPerIsland",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: 6,
                oldClrType: typeof(int),
                oldType: "INTEGER");

            migrationBuilder.AlterColumn<float>(
                name: "AutoSupportMaxLayerHeightMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 1.5f,
                oldClrType: typeof(float),
                oldType: "REAL");

            migrationBuilder.AlterColumn<float>(
                name: "AutoSupportBedMarginMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 2f,
                oldClrType: typeof(float),
                oldType: "REAL");
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AlterColumn<float>(
                name: "AutoSupportSphereRadiusMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                oldClrType: typeof(float),
                oldType: "REAL",
                oldDefaultValue: 1.2f);

            migrationBuilder.AlterColumn<float>(
                name: "AutoSupportPullForceThreshold",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                oldClrType: typeof(float),
                oldType: "REAL",
                oldDefaultValue: 3f);

            migrationBuilder.AlterColumn<float>(
                name: "AutoSupportMinVoxelSizeMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                oldClrType: typeof(float),
                oldType: "REAL",
                oldDefaultValue: 0.8f);

            migrationBuilder.AlterColumn<float>(
                name: "AutoSupportMinLayerHeightMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                oldClrType: typeof(float),
                oldType: "REAL",
                oldDefaultValue: 0.75f);

            migrationBuilder.AlterColumn<float>(
                name: "AutoSupportMergeDistanceMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                oldClrType: typeof(float),
                oldType: "REAL",
                oldDefaultValue: 2.5f);

            migrationBuilder.AlterColumn<float>(
                name: "AutoSupportMaxVoxelSizeMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                oldClrType: typeof(float),
                oldType: "REAL",
                oldDefaultValue: 2f);

            migrationBuilder.AlterColumn<int>(
                name: "AutoSupportMaxSupportsPerIsland",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                oldClrType: typeof(int),
                oldType: "INTEGER",
                oldDefaultValue: 6);

            migrationBuilder.AlterColumn<float>(
                name: "AutoSupportMaxLayerHeightMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                oldClrType: typeof(float),
                oldType: "REAL",
                oldDefaultValue: 1.5f);

            migrationBuilder.AlterColumn<float>(
                name: "AutoSupportBedMarginMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                oldClrType: typeof(float),
                oldType: "REAL",
                oldDefaultValue: 2f);
        }
    }
}
