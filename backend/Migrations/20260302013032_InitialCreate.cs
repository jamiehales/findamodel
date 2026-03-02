using System;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class InitialCreate : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.CreateTable(
                name: "DirectoryConfigs",
                columns: table => new
                {
                    Id = table.Column<Guid>(type: "TEXT", nullable: false),
                    DirectoryPath = table.Column<string>(type: "TEXT", nullable: false),
                    ParentId = table.Column<Guid>(type: "TEXT", nullable: true),
                    RawCreator = table.Column<string>(type: "TEXT", nullable: true),
                    RawCollection = table.Column<string>(type: "TEXT", nullable: true),
                    RawCategory = table.Column<string>(type: "TEXT", nullable: true),
                    RawType = table.Column<string>(type: "TEXT", nullable: true),
                    RawSupported = table.Column<bool>(type: "INTEGER", nullable: true),
                    RawSubcollection = table.Column<string>(type: "TEXT", nullable: true),
                    Creator = table.Column<string>(type: "TEXT", nullable: true),
                    Collection = table.Column<string>(type: "TEXT", nullable: true),
                    Subcollection = table.Column<string>(type: "TEXT", nullable: true),
                    Category = table.Column<string>(type: "TEXT", nullable: true),
                    Type = table.Column<string>(type: "TEXT", nullable: true),
                    Supported = table.Column<bool>(type: "INTEGER", nullable: true),
                    LocalConfigFileHash = table.Column<string>(type: "TEXT", nullable: true),
                    UpdatedAt = table.Column<DateTime>(type: "TEXT", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_DirectoryConfigs", x => x.Id);
                    table.ForeignKey(
                        name: "FK_DirectoryConfigs_DirectoryConfigs_ParentId",
                        column: x => x.ParentId,
                        principalTable: "DirectoryConfigs",
                        principalColumn: "Id",
                        onDelete: ReferentialAction.Restrict);
                });

            migrationBuilder.CreateTable(
                name: "Users",
                columns: table => new
                {
                    Id = table.Column<Guid>(type: "TEXT", nullable: false),
                    Username = table.Column<string>(type: "TEXT", nullable: false),
                    IsAdmin = table.Column<bool>(type: "INTEGER", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_Users", x => x.Id);
                });

            migrationBuilder.CreateTable(
                name: "Models",
                columns: table => new
                {
                    Id = table.Column<Guid>(type: "TEXT", nullable: false),
                    Checksum = table.Column<string>(type: "TEXT", nullable: false),
                    FileName = table.Column<string>(type: "TEXT", nullable: false),
                    Directory = table.Column<string>(type: "TEXT", nullable: false),
                    FileType = table.Column<string>(type: "TEXT", nullable: false),
                    FileSize = table.Column<long>(type: "INTEGER", nullable: false),
                    FileModifiedAt = table.Column<DateTime>(type: "TEXT", nullable: false),
                    CachedAt = table.Column<DateTime>(type: "TEXT", nullable: false),
                    PreviewImagePath = table.Column<string>(type: "TEXT", nullable: true),
                    PreviewGeneratedAt = table.Column<DateTime>(type: "TEXT", nullable: true),
                    DirectoryConfigId = table.Column<Guid>(type: "TEXT", nullable: true),
                    ConvexHullCoordinates = table.Column<string>(type: "TEXT", nullable: true),
                    ConcaveHullCoordinates = table.Column<string>(type: "TEXT", nullable: true),
                    ConvexSansRaftHullCoordinates = table.Column<string>(type: "TEXT", nullable: true),
                    HullGeneratedAt = table.Column<DateTime>(type: "TEXT", nullable: true),
                    DimensionXMm = table.Column<float>(type: "REAL", nullable: true),
                    DimensionYMm = table.Column<float>(type: "REAL", nullable: true),
                    DimensionZMm = table.Column<float>(type: "REAL", nullable: true),
                    SphereCentreX = table.Column<float>(type: "REAL", nullable: true),
                    SphereCentreY = table.Column<float>(type: "REAL", nullable: true),
                    SphereCentreZ = table.Column<float>(type: "REAL", nullable: true),
                    SphereRadius = table.Column<float>(type: "REAL", nullable: true),
                    GeometryCalculatedAt = table.Column<DateTime>(type: "TEXT", nullable: true)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_Models", x => x.Id);
                    table.ForeignKey(
                        name: "FK_Models_DirectoryConfigs_DirectoryConfigId",
                        column: x => x.DirectoryConfigId,
                        principalTable: "DirectoryConfigs",
                        principalColumn: "Id",
                        onDelete: ReferentialAction.SetNull);
                });

            migrationBuilder.CreateTable(
                name: "PrintingLists",
                columns: table => new
                {
                    Id = table.Column<Guid>(type: "TEXT", nullable: false),
                    Name = table.Column<string>(type: "TEXT", nullable: false),
                    OwnerId = table.Column<Guid>(type: "TEXT", nullable: false),
                    IsActive = table.Column<bool>(type: "INTEGER", nullable: false),
                    IsDefault = table.Column<bool>(type: "INTEGER", nullable: false),
                    CreatedAt = table.Column<DateTime>(type: "TEXT", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_PrintingLists", x => x.Id);
                    table.ForeignKey(
                        name: "FK_PrintingLists_Users_OwnerId",
                        column: x => x.OwnerId,
                        principalTable: "Users",
                        principalColumn: "Id",
                        onDelete: ReferentialAction.Cascade);
                });

            migrationBuilder.CreateTable(
                name: "PrintingListItems",
                columns: table => new
                {
                    Id = table.Column<Guid>(type: "TEXT", nullable: false),
                    PrintingListId = table.Column<Guid>(type: "TEXT", nullable: false),
                    ModelId = table.Column<Guid>(type: "TEXT", nullable: false),
                    Quantity = table.Column<int>(type: "INTEGER", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_PrintingListItems", x => x.Id);
                    table.ForeignKey(
                        name: "FK_PrintingListItems_PrintingLists_PrintingListId",
                        column: x => x.PrintingListId,
                        principalTable: "PrintingLists",
                        principalColumn: "Id",
                        onDelete: ReferentialAction.Cascade);
                });

            migrationBuilder.CreateIndex(
                name: "IX_DirectoryConfigs_DirectoryPath",
                table: "DirectoryConfigs",
                column: "DirectoryPath",
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_DirectoryConfigs_ParentId",
                table: "DirectoryConfigs",
                column: "ParentId");

            migrationBuilder.CreateIndex(
                name: "IX_Models_Checksum",
                table: "Models",
                column: "Checksum");

            migrationBuilder.CreateIndex(
                name: "IX_Models_DirectoryConfigId",
                table: "Models",
                column: "DirectoryConfigId");

            migrationBuilder.CreateIndex(
                name: "IX_PrintingListItems_PrintingListId",
                table: "PrintingListItems",
                column: "PrintingListId");

            migrationBuilder.CreateIndex(
                name: "IX_PrintingLists_OwnerId",
                table: "PrintingLists",
                column: "OwnerId");

            migrationBuilder.CreateIndex(
                name: "IX_Users_Username",
                table: "Users",
                column: "Username",
                unique: true);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropTable(
                name: "Models");

            migrationBuilder.DropTable(
                name: "PrintingListItems");

            migrationBuilder.DropTable(
                name: "DirectoryConfigs");

            migrationBuilder.DropTable(
                name: "PrintingLists");

            migrationBuilder.DropTable(
                name: "Users");
        }
    }
}
