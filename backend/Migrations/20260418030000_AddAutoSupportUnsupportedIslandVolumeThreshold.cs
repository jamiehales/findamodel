using findamodel.Data;
using Microsoft.EntityFrameworkCore.Infrastructure;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    [DbContext(typeof(ModelCacheContext))]
    [Migration("20260418030000_AddAutoSupportUnsupportedIslandVolumeThreshold")]
    public partial class AddAutoSupportUnsupportedIslandVolumeThreshold : Migration
    {
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<float>(
                name: "AutoSupportUnsupportedIslandVolumeThresholdMm3",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 1f);
        }

        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "AutoSupportUnsupportedIslandVolumeThresholdMm3",
                table: "AppConfigs");
        }
    }
}
