using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddCachedModelMetadata : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<string>(
                name: "CalculatedCategory",
                table: "Models",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "CalculatedCollection",
                table: "Models",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "CalculatedCreator",
                table: "Models",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "CalculatedModelName",
                table: "Models",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "CalculatedSubcollection",
                table: "Models",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<bool>(
                name: "CalculatedSupported",
                table: "Models",
                type: "INTEGER",
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "CalculatedType",
                table: "Models",
                type: "TEXT",
                nullable: true);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "CalculatedCategory",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "CalculatedCollection",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "CalculatedCreator",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "CalculatedModelName",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "CalculatedSubcollection",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "CalculatedSupported",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "CalculatedType",
                table: "Models");
        }
    }
}
