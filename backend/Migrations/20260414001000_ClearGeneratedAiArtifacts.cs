using Microsoft.EntityFrameworkCore.Infrastructure;
using Microsoft.EntityFrameworkCore.Migrations;
using findamodel.Data;

#nullable disable

namespace findamodel.Migrations
{
    [DbContext(typeof(ModelCacheContext))]
    [Migration("20260414001000_ClearGeneratedAiArtifacts")]
    public partial class ClearGeneratedAiArtifacts : Migration
    {
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.Sql(
                """
                UPDATE Models
                SET GeneratedTagsJson = NULL,
                    GeneratedTagsModel = NULL,
                    GeneratedTagsAt = NULL,
                    GeneratedTagsConfidenceJson = NULL,
                    GeneratedTagsChecksum = NULL,
                    GeneratedTagsStatus = 'none',
                    GeneratedTagsError = NULL,
                    GeneratedDescription = NULL,
                    GeneratedDescriptionModel = NULL,
                    GeneratedDescriptionAt = NULL,
                    GeneratedDescriptionChecksum = NULL;
                """);
        }

        protected override void Down(MigrationBuilder migrationBuilder)
        {
        }
    }
}
