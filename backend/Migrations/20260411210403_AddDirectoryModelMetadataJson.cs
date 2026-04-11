using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddDirectoryModelMetadataJson : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<string>(
                name: "CalculatedPartName",
                table: "Models",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "RawModelMetadataJson",
                table: "DirectoryConfigs",
                type: "TEXT",
                nullable: true);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "CalculatedPartName",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "RawModelMetadataJson",
                table: "DirectoryConfigs");
        }
    }
}
