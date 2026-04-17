using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddAutoSupportIslandAndDistanceSettings : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<float>(
                name: "AutoSupportMaxSupportDistanceMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 10f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportMinIslandAreaMm2",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 4f);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "AutoSupportMaxSupportDistanceMm",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportMinIslandAreaMm2",
                table: "AppConfigs");
        }
    }
}
