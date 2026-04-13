using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class ClearDirectoryConfigChecksums : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            // Clears all directory config checksums, forcing a full re-sync of
            // directory configs on next startup - which recomputes all Calculated*
            // fields (including CalculatedModelName) on every CachedModel record.
            migrationBuilder.Sql("UPDATE DirectoryConfigs SET LocalConfigFileHash = NULL");
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            // Original checksums cannot be restored.
        }
    }
}
