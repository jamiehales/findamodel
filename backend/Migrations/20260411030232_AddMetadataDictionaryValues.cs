using System;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddMetadataDictionaryValues : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            var seedCreatedAt = new DateTime(2026, 4, 10, 0, 0, 0, DateTimeKind.Utc);

            migrationBuilder.CreateTable(
                name: "MetadataDictionaryValues",
                columns: table => new
                {
                    Id = table.Column<Guid>(type: "TEXT", nullable: false),
                    Field = table.Column<string>(type: "TEXT", nullable: false),
                    Value = table.Column<string>(type: "TEXT", nullable: false),
                    NormalizedValue = table.Column<string>(type: "TEXT", nullable: false),
                    CreatedAt = table.Column<DateTime>(type: "TEXT", nullable: false),
                    UpdatedAt = table.Column<DateTime>(type: "TEXT", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_MetadataDictionaryValues", x => x.Id);
                });

            migrationBuilder.CreateIndex(
                name: "IX_MetadataDictionaryValues_Field_NormalizedValue",
                table: "MetadataDictionaryValues",
                columns: new[] { "Field", "NormalizedValue" },
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_MetadataDictionaryValues_Field_Value",
                table: "MetadataDictionaryValues",
                columns: new[] { "Field", "Value" });

            migrationBuilder.InsertData(
                table: "MetadataDictionaryValues",
                columns: new[] { "Id", "Field", "Value", "NormalizedValue", "CreatedAt", "UpdatedAt" },
                values: new object[,]
                {
                    { new Guid("70f5e4fb-7d9e-4f74-aec8-2038c2780bb6"), "category", "Bust", "bust", seedCreatedAt, seedCreatedAt },
                    { new Guid("487d2907-245f-4af9-8353-57507c63bd0f"), "category", "Miniature", "miniature", seedCreatedAt, seedCreatedAt },
                    { new Guid("cb0ea64c-b569-4301-b8eb-9534f4c7f395"), "category", "Uncategorized", "uncategorized", seedCreatedAt, seedCreatedAt },
                    { new Guid("34a684ad-a67c-4598-9d09-8a13d9036685"), "type", "Whole", "whole", seedCreatedAt, seedCreatedAt },
                    { new Guid("f47d5f95-c995-4025-bcb5-2ec644ca4d0a"), "type", "Part", "part", seedCreatedAt, seedCreatedAt },
                    { new Guid("8e89c7c7-f7c4-45de-97f7-08765c83f905"), "material", "FDM", "fdm", seedCreatedAt, seedCreatedAt },
                    { new Guid("e6034f5a-4919-4ac0-9488-7f42e2ec4dcf"), "material", "Resin", "resin", seedCreatedAt, seedCreatedAt },
                    { new Guid("71132eb0-f176-4513-b44e-88f7502675dc"), "material", "All", "all", seedCreatedAt, seedCreatedAt },
                });
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropTable(
                name: "MetadataDictionaryValues");
        }
    }
}
