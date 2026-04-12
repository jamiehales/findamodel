using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class RemoveTagGenerationAutoApply : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "TagGenerationAutoApply",
                table: "AppConfigs");
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<bool>(
                name: "TagGenerationAutoApply",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: true);
        }
    }
}
