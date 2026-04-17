using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddPrinterPixelResolution : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<int>(
                name: "PixelHeight",
                table: "PrinterConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: 4320);

            migrationBuilder.AddColumn<int>(
                name: "PixelWidth",
                table: "PrinterConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: 7680);

            migrationBuilder.UpdateData(
                table: "PrinterConfigs",
                keyColumn: "Id",
                keyValue: new Guid("8a04c3c1-f5e0-4d95-a7b8-2c9d1e3f4a5b"),
                columns: ["PixelWidth", "PixelHeight"],
                values: new object[] { 7680, 4320 });

            migrationBuilder.UpdateData(
                table: "PrinterConfigs",
                keyColumn: "Id",
                keyValue: new Guid("9b15d4d2-a6f1-5e06-b8c9-3d0e2f4e5b6c"),
                columns: ["PixelWidth", "PixelHeight"],
                values: new object[] { 11520, 5120 });
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "PixelHeight",
                table: "PrinterConfigs");

            migrationBuilder.DropColumn(
                name: "PixelWidth",
                table: "PrinterConfigs");
        }
    }
}
