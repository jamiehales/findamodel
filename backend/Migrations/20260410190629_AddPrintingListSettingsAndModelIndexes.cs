using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddPrintingListSettingsAndModelIndexes : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<string>(
                name: "HullMode",
                table: "PrintingLists",
                type: "TEXT",
                nullable: false,
                defaultValue: "convex");

            migrationBuilder.AddColumn<string>(
                name: "SpawnType",
                table: "PrintingLists",
                type: "TEXT",
                nullable: false,
                defaultValue: "grouped");

            migrationBuilder.CreateIndex(
                name: "IX_Models_CalculatedCreator_CalculatedCollection_CalculatedSubcollection_CalculatedModelName",
                table: "Models",
                columns: new[] { "CalculatedCreator", "CalculatedCollection", "CalculatedSubcollection", "CalculatedModelName" });

            migrationBuilder.CreateIndex(
                name: "IX_Models_Directory_FileName",
                table: "Models",
                columns: new[] { "Directory", "FileName" },
                unique: true);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropIndex(
                name: "IX_Models_CalculatedCreator_CalculatedCollection_CalculatedSubcollection_CalculatedModelName",
                table: "Models");

            migrationBuilder.DropIndex(
                name: "IX_Models_Directory_FileName",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "HullMode",
                table: "PrintingLists");

            migrationBuilder.DropColumn(
                name: "SpawnType",
                table: "PrintingLists");
        }
    }
}
