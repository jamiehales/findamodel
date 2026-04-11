using System;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddRaftHeightConfigAndAppConfig : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.RenameColumn(
                name: "HullRaftOffsetMm",
                table: "Models",
                newName: "HullRaftHeightMm");

            migrationBuilder.AddColumn<float>(
                name: "RaftHeightMm",
                table: "DirectoryConfigs",
                type: "REAL",
                nullable: true);

            migrationBuilder.AddColumn<float>(
                name: "RawRaftHeightMm",
                table: "DirectoryConfigs",
                type: "REAL",
                nullable: true);

            migrationBuilder.CreateTable(
                name: "AppConfigs",
                columns: table => new
                {
                    Id = table.Column<int>(type: "INTEGER", nullable: false)
                        .Annotation("Sqlite:Autoincrement", true),
                    DefaultRaftHeightMm = table.Column<float>(type: "REAL", nullable: false, defaultValue: 2f),
                    UpdatedAt = table.Column<DateTime>(type: "TEXT", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_AppConfigs", x => x.Id);
                });
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropTable(
                name: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "RaftHeightMm",
                table: "DirectoryConfigs");

            migrationBuilder.DropColumn(
                name: "RawRaftHeightMm",
                table: "DirectoryConfigs");

            migrationBuilder.RenameColumn(
                name: "HullRaftHeightMm",
                table: "Models",
                newName: "HullRaftOffsetMm");
        }
    }
}
