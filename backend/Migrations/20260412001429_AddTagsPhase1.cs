using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddTagsPhase1 : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<string>(
                name: "CalculatedTagsJson",
                table: "Models",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "RawTagsJson",
                table: "DirectoryConfigs",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "TagsJson",
                table: "DirectoryConfigs",
                type: "TEXT",
                nullable: true);

            migrationBuilder.CreateIndex(
                name: "IX_Models_CalculatedTagsJson",
                table: "Models",
                column: "CalculatedTagsJson");
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropIndex(
                name: "IX_Models_CalculatedTagsJson",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "CalculatedTagsJson",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "RawTagsJson",
                table: "DirectoryConfigs");

            migrationBuilder.DropColumn(
                name: "TagsJson",
                table: "DirectoryConfigs");
        }
    }
}
