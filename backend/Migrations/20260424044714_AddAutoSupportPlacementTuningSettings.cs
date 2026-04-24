using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddAutoSupportPlacementTuningSettings : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<bool>(
                name: "AutoSupportAccessibilityEnabled",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: true);

            migrationBuilder.AddColumn<int>(
                name: "AutoSupportAccessibilityMinOpenDirections",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: 1);

            migrationBuilder.AddColumn<int>(
                name: "AutoSupportAccessibilityScanRadiusPx",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: 6);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportBridgeReductionFactor",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0.3f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportCantileverMomentMultiplier",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0.4f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportCantileverReferenceLengthMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 8f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportDrainageDepthForceMultiplier",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0.15f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportHeightBias",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0.3f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportLayerAdhesionSafetyFactor",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 1.1f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportLayerBondStrengthPerMm2",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 1.2f);

            migrationBuilder.AddColumn<bool>(
                name: "AutoSupportOrientationCheckEnabled",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: true);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportOrientationRiskForceMultiplierMax",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 1.35f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportOrientationRiskThresholdRatio",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 1.15f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportOverhangSensitivity",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0.65f);

            migrationBuilder.AddColumn<int>(
                name: "AutoSupportPeelDirection",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: 2);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportPeelEndMultiplier",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0.9f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportPeelStartMultiplier",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 1.3f);

            migrationBuilder.AddColumn<bool>(
                name: "AutoSupportSupportInteractionEnabled",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: true);

            migrationBuilder.AddColumn<int>(
                name: "AutoSupportSurfaceQualitySearchRadiusPx",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: 6);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportSurfaceQualityWeight",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0.35f);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "AutoSupportAccessibilityEnabled",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportAccessibilityMinOpenDirections",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportAccessibilityScanRadiusPx",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportBridgeReductionFactor",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportCantileverMomentMultiplier",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportCantileverReferenceLengthMm",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportDrainageDepthForceMultiplier",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportHeightBias",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportLayerAdhesionSafetyFactor",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportLayerBondStrengthPerMm2",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportOrientationCheckEnabled",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportOrientationRiskForceMultiplierMax",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportOrientationRiskThresholdRatio",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportOverhangSensitivity",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportPeelDirection",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportPeelEndMultiplier",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportPeelStartMultiplier",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportSupportInteractionEnabled",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportSurfaceQualitySearchRadiusPx",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportSurfaceQualityWeight",
                table: "AppConfigs");
        }
    }
}
