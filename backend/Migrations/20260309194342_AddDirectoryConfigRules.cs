using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddDirectoryConfigRules : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<string>(
                name: "RawRulesJson",
                table: "DirectoryConfigs",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "ResolvedRulesJson",
                table: "DirectoryConfigs",
                type: "TEXT",
                nullable: true);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "RawRulesJson",
                table: "DirectoryConfigs");

            migrationBuilder.DropColumn(
                name: "ResolvedRulesJson",
                table: "DirectoryConfigs");
        }
    }
}
