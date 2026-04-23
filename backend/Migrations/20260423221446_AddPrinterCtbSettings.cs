using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace findamodel.Migrations
{
    /// <inheritdoc />
    public partial class AddPrinterCtbSettings : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<float>(
                name: "BottomExposureTimeSeconds",
                table: "PrinterConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 30f);

            migrationBuilder.AddColumn<int>(
                name: "BottomLayerCount",
                table: "PrinterConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: 4);

            migrationBuilder.AddColumn<float>(
                name: "BottomLiftHeightMm",
                table: "PrinterConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 6f);

            migrationBuilder.AddColumn<float>(
                name: "BottomLiftSpeedMmPerMinute",
                table: "PrinterConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 65f);

            migrationBuilder.AddColumn<float>(
                name: "BottomLightOffDelaySeconds",
                table: "PrinterConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0f);

            migrationBuilder.AddColumn<byte>(
                name: "BottomLightPwm",
                table: "PrinterConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: (byte)255);

            migrationBuilder.AddColumn<float>(
                name: "ExposureTimeSeconds",
                table: "PrinterConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 2.5f);

            migrationBuilder.AddColumn<float>(
                name: "LayerHeightMm",
                table: "PrinterConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0.05f);

            migrationBuilder.AddColumn<float>(
                name: "LiftHeightMm",
                table: "PrinterConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 6f);

            migrationBuilder.AddColumn<float>(
                name: "LiftSpeedMmPerMinute",
                table: "PrinterConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 80f);

            migrationBuilder.AddColumn<float>(
                name: "LightOffDelaySeconds",
                table: "PrinterConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0f);

            migrationBuilder.AddColumn<byte>(
                name: "LightPwm",
                table: "PrinterConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: (byte)255);

            migrationBuilder.AddColumn<float>(
                name: "RetractSpeedMmPerMinute",
                table: "PrinterConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 150f);

            migrationBuilder.AddColumn<int>(
                name: "TransitionLayerCount",
                table: "PrinterConfigs",
                type: "INTEGER",
                nullable: false,
                defaultValue: 0);

            migrationBuilder.AddColumn<float>(
                name: "WaitTimeAfterCureSeconds",
                table: "PrinterConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0f);

            migrationBuilder.AddColumn<float>(
                name: "WaitTimeAfterLiftSeconds",
                table: "PrinterConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0f);

            migrationBuilder.AddColumn<float>(
                name: "WaitTimeBeforeCureSeconds",
                table: "PrinterConfigs",
                type: "REAL",
                nullable: false,
                defaultValue: 0f);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "BottomExposureTimeSeconds",
                table: "PrinterConfigs");

            migrationBuilder.DropColumn(
                name: "BottomLayerCount",
                table: "PrinterConfigs");

            migrationBuilder.DropColumn(
                name: "BottomLiftHeightMm",
                table: "PrinterConfigs");

            migrationBuilder.DropColumn(
                name: "BottomLiftSpeedMmPerMinute",
                table: "PrinterConfigs");

            migrationBuilder.DropColumn(
                name: "BottomLightOffDelaySeconds",
                table: "PrinterConfigs");

            migrationBuilder.DropColumn(
                name: "BottomLightPwm",
                table: "PrinterConfigs");

            migrationBuilder.DropColumn(
                name: "ExposureTimeSeconds",
                table: "PrinterConfigs");

            migrationBuilder.DropColumn(
                name: "LayerHeightMm",
                table: "PrinterConfigs");

            migrationBuilder.DropColumn(
                name: "LiftHeightMm",
                table: "PrinterConfigs");

            migrationBuilder.DropColumn(
                name: "LiftSpeedMmPerMinute",
                table: "PrinterConfigs");

            migrationBuilder.DropColumn(
                name: "LightOffDelaySeconds",
                table: "PrinterConfigs");

            migrationBuilder.DropColumn(
                name: "LightPwm",
                table: "PrinterConfigs");

            migrationBuilder.DropColumn(
                name: "RetractSpeedMmPerMinute",
                table: "PrinterConfigs");

            migrationBuilder.DropColumn(
                name: "TransitionLayerCount",
                table: "PrinterConfigs");

            migrationBuilder.DropColumn(
                name: "WaitTimeAfterCureSeconds",
                table: "PrinterConfigs");

            migrationBuilder.DropColumn(
                name: "WaitTimeAfterLiftSeconds",
                table: "PrinterConfigs");

            migrationBuilder.DropColumn(
                name: "WaitTimeBeforeCureSeconds",
                table: "PrinterConfigs");
        }
    }
}
