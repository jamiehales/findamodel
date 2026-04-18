using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddAutoSupportTipSizesAndResinStrength : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<float>(
                name: "AutoSupportResinStrength",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 1f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportMicroTipRadiusMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0.4f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportLightTipRadiusMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0.7f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportMediumTipRadiusMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 1f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportHeavyTipRadiusMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 1.5f);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "AutoSupportResinStrength",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportMicroTipRadiusMm",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportLightTipRadiusMm",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportMediumTipRadiusMm",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportHeavyTipRadiusMm",
                table: "AppConfigs");
        }
    }
}
