using System;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddIndexRunHistory : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.CreateTable(
                name: "IndexRuns",
                columns: table => new
                {
                    Id = table.Column<Guid>(type: "TEXT", nullable: false),
                    DirectoryFilter = table.Column<string>(type: "TEXT", nullable: true),
                    RelativeModelPath = table.Column<string>(type: "TEXT", nullable: true),
                    Flags = table.Column<int>(type: "INTEGER", nullable: false),
                    RequestedAt = table.Column<DateTime>(type: "TEXT", nullable: false),
                    StartedAt = table.Column<DateTime>(type: "TEXT", nullable: true),
                    CompletedAt = table.Column<DateTime>(type: "TEXT", nullable: true),
                    DurationMs = table.Column<double>(type: "REAL", nullable: true),
                    Status = table.Column<string>(type: "TEXT", nullable: false),
                    Outcome = table.Column<string>(type: "TEXT", nullable: true),
                    Error = table.Column<string>(type: "TEXT", nullable: true),
                    TotalFiles = table.Column<int>(type: "INTEGER", nullable: true),
                    ProcessedFiles = table.Column<int>(type: "INTEGER", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_IndexRuns", x => x.Id);
                });

            migrationBuilder.CreateTable(
                name: "IndexRunEvents",
                columns: table => new
                {
                    Id = table.Column<Guid>(type: "TEXT", nullable: false),
                    IndexRunId = table.Column<Guid>(type: "TEXT", nullable: false),
                    CreatedAt = table.Column<DateTime>(type: "TEXT", nullable: false),
                    Level = table.Column<string>(type: "TEXT", nullable: false),
                    Message = table.Column<string>(type: "TEXT", nullable: false),
                    RelativePath = table.Column<string>(type: "TEXT", nullable: true)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_IndexRunEvents", x => x.Id);
                    table.ForeignKey(
                        name: "FK_IndexRunEvents_IndexRuns_IndexRunId",
                        column: x => x.IndexRunId,
                        principalTable: "IndexRuns",
                        principalColumn: "Id",
                        onDelete: ReferentialAction.Cascade);
                });

            migrationBuilder.CreateTable(
                name: "IndexRunFiles",
                columns: table => new
                {
                    Id = table.Column<Guid>(type: "TEXT", nullable: false),
                    IndexRunId = table.Column<Guid>(type: "TEXT", nullable: false),
                    RelativePath = table.Column<string>(type: "TEXT", nullable: false),
                    FileType = table.Column<string>(type: "TEXT", nullable: false),
                    Status = table.Column<string>(type: "TEXT", nullable: false),
                    IsNew = table.Column<bool>(type: "INTEGER", nullable: false),
                    WasUpdated = table.Column<bool>(type: "INTEGER", nullable: false),
                    GeneratedPreview = table.Column<bool>(type: "INTEGER", nullable: false),
                    GeneratedHull = table.Column<bool>(type: "INTEGER", nullable: false),
                    GeneratedAiTags = table.Column<bool>(type: "INTEGER", nullable: false),
                    GeneratedAiDescription = table.Column<bool>(type: "INTEGER", nullable: false),
                    Message = table.Column<string>(type: "TEXT", nullable: true),
                    ProcessedAt = table.Column<DateTime>(type: "TEXT", nullable: true)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_IndexRunFiles", x => x.Id);
                    table.ForeignKey(
                        name: "FK_IndexRunFiles_IndexRuns_IndexRunId",
                        column: x => x.IndexRunId,
                        principalTable: "IndexRuns",
                        principalColumn: "Id",
                        onDelete: ReferentialAction.Cascade);
                });

            migrationBuilder.CreateIndex(
                name: "IX_IndexRunEvents_IndexRunId_CreatedAt",
                table: "IndexRunEvents",
                columns: new[] { "IndexRunId", "CreatedAt" });

            migrationBuilder.CreateIndex(
                name: "IX_IndexRunFiles_IndexRunId_RelativePath",
                table: "IndexRunFiles",
                columns: new[] { "IndexRunId", "RelativePath" },
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_IndexRuns_RequestedAt",
                table: "IndexRuns",
                column: "RequestedAt");

            migrationBuilder.CreateIndex(
                name: "IX_IndexRuns_Status",
                table: "IndexRuns",
                column: "Status");
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropTable(
                name: "IndexRunEvents");

            migrationBuilder.DropTable(
                name: "IndexRunFiles");

            migrationBuilder.DropTable(
                name: "IndexRuns");
        }
    }
}
