using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddMinimumPreviewGenerationVersion : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<int>(
                name: "MinimumPreviewGenerationVersion",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: 8);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "MinimumPreviewGenerationVersion",
                table: "AppConfigs");
        }
    }
}
