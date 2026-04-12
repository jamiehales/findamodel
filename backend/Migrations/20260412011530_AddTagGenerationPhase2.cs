using System;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddTagGenerationPhase2 : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<DateTime>(
                name: "GeneratedTagsAt",
                table: "Models",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "GeneratedTagsConfidenceJson",
                table: "Models",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "GeneratedTagsError",
                table: "Models",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "GeneratedTagsJson",
                table: "Models",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "GeneratedTagsModel",
                table: "Models",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "GeneratedTagsStatus",
                table: "Models",
                type: "TEXT",
                nullable: true);

            migrationBuilder.AddColumn<bool>(
                name: "TagGenerationAutoApply",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: true);

            migrationBuilder.AddColumn<bool>(
                name: "TagGenerationEnabled",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: false);

            migrationBuilder.AddColumn<string>(
                name: "TagGenerationEndpoint",
                table: "AppConfigs",
                type: "TEXT",
                nullable: false,
                defaultValue: "http://localhost:11434");

            migrationBuilder.AddColumn<int>(
                name: "TagGenerationMaxTags",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: 12);

            migrationBuilder.AddColumn<float>(
                name: "TagGenerationMinConfidence",
                table: "AppConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0.45f);

            migrationBuilder.AddColumn<string>(
                name: "TagGenerationModel",
                table: "AppConfigs",
                type: "TEXT",
                nullable: false,
                defaultValue: "qwen2.5vl:7b");

            migrationBuilder.AddColumn<string>(
                name: "TagGenerationProvider",
                table: "AppConfigs",
                type: "TEXT",
                nullable: false,
                defaultValue: "internal");

            migrationBuilder.AddColumn<int>(
                name: "TagGenerationTimeoutMs",
                table: "AppConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: 60000);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "GeneratedTagsAt",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "GeneratedTagsConfidenceJson",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "GeneratedTagsError",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "GeneratedTagsJson",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "GeneratedTagsModel",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "GeneratedTagsStatus",
                table: "Models");

            migrationBuilder.DropColumn(
                name: "TagGenerationAutoApply",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "TagGenerationEnabled",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "TagGenerationEndpoint",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "TagGenerationMaxTags",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "TagGenerationMinConfidence",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "TagGenerationModel",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "TagGenerationProvider",
                table: "AppConfigs");

            migrationBuilder.DropColumn(
                name: "TagGenerationTimeoutMs",
                table: "AppConfigs");
        }
    }
}
