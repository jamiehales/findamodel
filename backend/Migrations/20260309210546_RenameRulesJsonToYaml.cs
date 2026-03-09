using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class RenameRulesJsonToYaml : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.RenameColumn(
                name: "ResolvedRulesJson",
                table: "DirectoryConfigs",
                newName: "ResolvedRulesYaml");

            migrationBuilder.RenameColumn(
                name: "RawRulesJson",
                table: "DirectoryConfigs",
                newName: "RawRulesYaml");
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.RenameColumn(
                name: "ResolvedRulesYaml",
                table: "DirectoryConfigs",
                newName: "ResolvedRulesJson");

            migrationBuilder.RenameColumn(
                name: "RawRulesYaml",
                table: "DirectoryConfigs",
                newName: "RawRulesJson");
        }
    }
}
