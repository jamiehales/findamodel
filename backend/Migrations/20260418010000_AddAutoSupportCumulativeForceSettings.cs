using Microsoft.EntityFrameworkCore.Migrations;
using Microsoft.EntityFrameworkCore.Infrastructure;
using findamodel.Data;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    [DbContext(typeof(ModelCacheContext))]
    [Migration("20260418010000_AddAutoSupportCumulativeForceSettings")]
    public partial class AddAutoSupportCumulativeForceSettings : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<float>(
                name: "AutoSupportResinDensityGPerMl",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 1.25f);

            migrationBuilder.AddColumn<float>(
                name: "AutoSupportPeelForceMultiplier",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0.15f);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "AutoSupportResinDensityGPerMl",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "AutoSupportPeelForceMultiplier",
                table: "AppConfigs");
        }
    }
}
