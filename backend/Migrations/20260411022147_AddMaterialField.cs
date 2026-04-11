using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddMaterialField : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<string>(
                name: "CalculatedMaterial",
                table: "Models",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "Material",
                table: "DirectoryConfigs",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "RawMaterial",
                table: "DirectoryConfigs",
                type: "TEXT",
                nullable: true);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "CalculatedMaterial",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "Material",
                table: "DirectoryConfigs");

            migrationBuilder.DropColumn(
                name: "RawMaterial",
                table: "DirectoryConfigs");
        }
    }
}
