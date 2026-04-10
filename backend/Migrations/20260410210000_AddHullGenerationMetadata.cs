using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddHullGenerationMetadata : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<int>(
                name: "HullGenerationVersion",
                table: "Models",
                type: "INTEGER",
                nullable: true);

            migrationBuilder.AddColumn<float>(
                name: "HullRaftOffsetMm",
                table: "Models",
                type: "REAL",
                nullable: true);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "HullGenerationVersion",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "HullRaftOffsetMm",
                table: "Models");
        }
    }
}
