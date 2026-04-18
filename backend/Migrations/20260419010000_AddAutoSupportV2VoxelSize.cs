using findamodel.Data;
using Microsoft.EntityFrameworkCore.Infrastructure;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    [DbContext(typeof(ModelCacheContext))]
    [Migration("20260419010000_AddAutoSupportV2VoxelSize")]
    public partial class AddAutoSupportV2VoxelSize : Migration
    {
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<float>(
                name: "AutoSupportV2VoxelSizeMm",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 2f);
        }

        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "AutoSupportV2VoxelSizeMm",
                table: "AppConfigs");
        }
    }
}
