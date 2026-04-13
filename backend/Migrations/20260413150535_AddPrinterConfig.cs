using System;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddPrinterConfig : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<Guid>(
                name: "PrinterConfigId",
                table: "PrintingLists",
                type: "TEXT",
                nullable: true);

            migrationBuilder.CreateTable(
                name: "PrinterConfigs",
                columns: table => new
                {
                    Id = table.Column<Guid>(type: "TEXT", nullable: false),
                    Name = table.Column<string>(type: "TEXT", nullable: false),
                    BedWidthMm = table.Column<float>(type: "REAL", nullable: false),
                    BedDepthMm = table.Column<float>(type: "REAL", nullable: false),
                    IsBuiltIn = table.Column<bool>(type: "INTEGER", nullable: false),
                    IsDefault = table.Column<bool>(type: "INTEGER", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_PrinterConfigs", x => x.Id);
                });

            migrationBuilder.CreateIndex(
                name: "IX_PrintingLists_PrinterConfigId",
                table: "PrintingLists",
                column: "PrinterConfigId");

            migrationBuilder.CreateIndex(
                name: "IX_PrinterConfigs_IsDefault",
                table: "PrinterConfigs",
                column: "IsDefault");

            migrationBuilder.AddForeignKey(
                name: "FK_PrintingLists_PrinterConfigs_PrinterConfigId",
                table: "PrintingLists",
                column: "PrinterConfigId",
                principalTable: "PrinterConfigs",
                principalColumn: "Id",
                onDelete: ReferentialAction.SetNull);

            migrationBuilder.InsertData(
                table: "PrinterConfigs",
                columns: new[] { "Id", "Name", "BedWidthMm", "BedDepthMm", "IsBuiltIn", "IsDefault" },
                values: new object[,]
                {
                    { new Guid("00000000-0000-0000-0000-000000000001"), "Uniformation GK2", 228f, 128f, true, true },
                });
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropForeignKey(
                name: "FK_PrintingLists_PrinterConfigs_PrinterConfigId",
                table: "PrintingLists");

            migrationBuilder.DropTable(
                name: "PrinterConfigs");

            migrationBuilder.DropIndex(
                name: "IX_PrintingLists_PrinterConfigId",
                table: "PrintingLists");

            migrationBuilder.DropColumn(
                name: "PrinterConfigId",
                table: "PrintingLists");
        }
    }
}
