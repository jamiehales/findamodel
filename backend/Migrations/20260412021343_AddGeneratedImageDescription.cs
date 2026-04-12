using System;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddGeneratedImageDescription : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<string>(
                name: "GeneratedDescription",
                table: "Models",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<DateTime>(
                name: "GeneratedDescriptionAt",
                table: "Models",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "GeneratedDescriptionChecksum",
                table: "Models",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "GeneratedDescriptionModel",
                table: "Models",
                type: "TEXT",
                nullable: true);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "GeneratedDescription",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "GeneratedDescriptionAt",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "GeneratedDescriptionChecksum",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "GeneratedDescriptionModel",
                table: "Models");
        }
    }
}
