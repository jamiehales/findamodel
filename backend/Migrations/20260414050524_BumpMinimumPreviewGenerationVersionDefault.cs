using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class BumpMinimumPreviewGenerationVersionDefault : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AlterColumn<int>(
                name: "MinimumPreviewGenerationVersion",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: 9,
                oldClrType: typeof(int),
                oldType: "INTEGER",
                oldDefaultValue: 8);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AlterColumn<int>(
                name: "MinimumPreviewGenerationVersion",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: 8,
                oldClrType: typeof(int),
                oldType: "INTEGER",
                oldDefaultValue: 9);
        }
    }
}
