using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddAutoSupportV3ForceSettings : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<float>(
                name: "AutoSupportAreaGrowthMultiplier",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportAreaGrowthThreshold",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportDragCoefficientMultiplier",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0f);

            migrationBuilder.AddColumn<bool>(
                name: "AutoSupportGravityEnabled",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: false);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportMinFeatureWidthMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportShrinkageEdgeBias",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportShrinkagePercent",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportSuctionMultiplier",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0f);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "AutoSupportAreaGrowthMultiplier",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportAreaGrowthThreshold",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportDragCoefficientMultiplier",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportGravityEnabled",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportMinFeatureWidthMm",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportShrinkageEdgeBias",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportShrinkagePercent",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportSuctionMultiplier",
                table: "AppConfigs");
        }
    }
}
