using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddPreviewGenerationVersion : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<int>(
                name: "PreviewGenerationVersion",
                table: "Models",
                type: "INTEGER",
                nullable: true);

            // Existing rows get PreviewGenerationVersion = NULL, which the scan treats as stale
            // and will regenerate on next index pass. The existing preview image path is preserved
            // so stale previews are still served until regenerated.
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "PreviewGenerationVersion",
                table: "Models");
        }
    }
}
