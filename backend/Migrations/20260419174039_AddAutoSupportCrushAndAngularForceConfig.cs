using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddAutoSupportCrushAndAngularForceConfig : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<float>(
                name: "AutoSupportCrushForceThreshold",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 20f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportMaxAngularForce",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 40f);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "AutoSupportCrushForceThreshold",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportMaxAngularForce",
                table: "AppConfigs");
        }
    }
}
