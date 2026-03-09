using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddDirectoryConfigModelName : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<string>(
                name: "ModelName",
                table: "DirectoryConfigs",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "RawModelName",
                table: "DirectoryConfigs",
                type: "TEXT",
                nullable: true);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "ModelName",
                table: "DirectoryConfigs");

            migrationBuilder.DropColumn(
                name: "RawModelName",
                table: "DirectoryConfigs");
        }
    }
}
