using System;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddUniformationGK3 : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            // Fix GK2 ID from temporary placeholder to proper GUID
            migrationBuilder.UpdateData(
                table: "PrinterConfigs",
                keyColumn: "Id",
                keyValue: new Guid("00000000-0000-0000-0000-000000000001"),
                column: "Id",
                value: new Guid("8a04c3c1-f5e0-4d95-a7b8-2c9d1e3f4a5b"));

            // Insert GK3 with IsDefault=true
            migrationBuilder.InsertData(
                table: "PrinterConfigs",
                columns: new[] { "Id", "Name", "BedWidthMm", "BedDepthMm", "IsBuiltIn", "IsDefault" },
                values: new object[,]
                {
                    { new Guid("9b15d4d2-a6f1-5e06-b8c9-3d0e2f4e5b6c"), "Uniformation GK3", 312f, 180f, true, true },
                });

            // Update GK2 to IsDefault=false since GK3 is now the default
            migrationBuilder.UpdateData(
                table: "PrinterConfigs",
                keyColumn: "Id",
                keyValue: new Guid("8a04c3c1-f5e0-4d95-a7b8-2c9d1e3f4a5b"),
                column: "IsDefault",
                value: false);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            // Delete GK3
            migrationBuilder.DeleteData(
                table: "PrinterConfigs",
                keyColumn: "Id",
                keyValue: new Guid("9b15d4d2-a6f1-5e06-b8c9-3d0e2f4e5b6c"));

            // Restore GK2 as the default
            migrationBuilder.UpdateData(
                table: "PrinterConfigs",
                keyColumn: "Id",
                keyValue: new Guid("8a04c3c1-f5e0-4d95-a7b8-2c9d1e3f4a5b"),
                column: "IsDefault",
                value: true);

            // Revert GK2 ID to temporary placeholder
            migrationBuilder.UpdateData(
                table: "PrinterConfigs",
                keyColumn: "Id",
                keyValue: new Guid("8a04c3c1-f5e0-4d95-a7b8-2c9d1e3f4a5b"),
                column: "Id",
                value: new Guid("00000000-0000-0000-0000-000000000001"));
        }
    }
}
