using System.Buffers.Binary;
using System.Security.Cryptography;
using System.Text;
using findamodel.Models;

namespace findamodel.Services;

public sealed class CtbExportService(PlateSliceRasterService sliceRasterService)
{
    private const uint Magic = 0x12FD0107;
    private const uint Version = 5;
    private const uint HeaderSize = 48;
    private const uint SettingsSize = 288;
    private const uint LayerPointerSize = 16;
    private const uint LayerDefinitionSize = 88;
    private const uint UnknownFooterValueV5 = 1109414650;
    private const uint UnknownFooterValue = 1833054899;
    private const string Disclaimer = "Layout and record format for the ctb and cbddlp file types are the copyrighted programs or codes of CBD Technology (China) Inc..The Customer or User shall not in any manner reproduce, distribute, modify, decompile, disassemble, decrypt, extract, reverse engineer, lease, assign, or sublicense the said programs or codes.";

    public byte[] GenerateFile(
        IReadOnlyList<IReadOnlyList<Triangle3D>> triangleGroups,
        PrinterConfigDto printer,
        IPlateGenerationProgressReporter? progressReporter,
        CancellationToken cancellationToken)
    {
        var layerHeightMm = printer.LayerHeightMm;
        if (layerHeightMm <= 0)
            throw new ArgumentException("Layer height must be positive for CTB export.");

        var nonEmptyGroups = triangleGroups.Where(g => g.Count > 0).ToArray();
        var maxY = nonEmptyGroups.Length == 0
            ? 0f
            : nonEmptyGroups.Max(group => group.Max(t => MathF.Max(t.V0.Y, MathF.Max(t.V1.Y, t.V2.Y))));
        var layerCount = Math.Max(1, (int)Math.Ceiling(Math.Max(0f, maxY) / layerHeightMm));

        progressReporter?.StartStage(layerCount, "Rendering CTB layers");

        var layerPayloads = new byte[layerCount][];
        for (var layerIndex = 0; layerIndex < layerCount; layerIndex++)
        {
            cancellationToken.ThrowIfCancellationRequested();
            progressReporter?.MarkCurrentEntry($"Rendering layer {layerIndex + 1} of {layerCount}");

            var sliceHeightMm = (layerIndex * layerHeightMm) + (layerHeightMm * 0.5f);
            var bitmap = sliceRasterService.RenderLayerBitmap(
                triangleGroups,
                sliceHeightMm,
                printer.BedWidthMm,
                printer.BedDepthMm,
                printer.PixelWidth,
                printer.PixelHeight,
                PngSliceExportMethod.MeshIntersection,
                layerHeightMm);

            layerPayloads[layerIndex] = EncodeLayerRle(bitmap.Pixels);
            progressReporter?.MarkEntryCompleted();
        }

        using var ms = new MemoryStream();
        using var writer = new BinaryWriter(ms, Encoding.UTF8, leaveOpen: true);

        WriteHeaderPlaceholder(writer);
        var settingsOffset = writer.BaseStream.Position;
        writer.Write(new byte[SettingsSize]);

        var machineNameOffset = (uint)writer.BaseStream.Position;
        var machineNameBytes = Encoding.UTF8.GetBytes(printer.Name);
        writer.Write(machineNameBytes);

        var disclaimerOffset = (uint)writer.BaseStream.Position;
        var disclaimerBytes = Encoding.UTF8.GetBytes(Disclaimer);
        if (disclaimerBytes.Length >= 320)
            writer.Write(disclaimerBytes, 0, 320);
        else
        {
            writer.Write(disclaimerBytes);
            writer.Write(new byte[320 - disclaimerBytes.Length]);
        }

        var layerTableOffset = (uint)writer.BaseStream.Position;
        var layerPointerTableLength = checked((int)(LayerPointerSize * (uint)layerCount));
        writer.Write(new byte[layerPointerTableLength]);

        var layerPointers = new (uint Offset, uint PageNumber)[layerCount];

        for (var layerIndex = 0; layerIndex < layerCount; layerIndex++)
        {
            cancellationToken.ThrowIfCancellationRequested();

            var payload = layerPayloads[layerIndex];
            var layerDefOffset = (uint)writer.BaseStream.Position;
            var layerDataOffset = layerDefOffset + LayerDefinitionSize;
            layerPointers[layerIndex] = (layerDefOffset, 0);

            WriteLayerDefinition(writer, layerIndex, layerDataOffset, payload.Length, printer);
            writer.Write(payload);
        }

        if (Version >= 5)
        {
            writer.Write(UnknownFooterValueV5);
            writer.Write(0u);
        }

        var signatureOffset = (uint)writer.BaseStream.Position;
        var checksumBytes = SHA256.HashData(BitConverter.GetBytes(DateTimeOffset.UtcNow.ToUnixTimeSeconds()));
        writer.Write(checksumBytes);
        writer.Write(UnknownFooterValue);

        var fileLength = writer.BaseStream.Position;

        writer.BaseStream.Seek(layerTableOffset, SeekOrigin.Begin);
        for (var i = 0; i < layerPointers.Length; i++)
        {
            writer.Write(layerPointers[i].Offset);
            writer.Write(layerPointers[i].PageNumber);
            writer.Write(LayerDefinitionSize);
            writer.Write(0u);
        }

        writer.BaseStream.Seek(settingsOffset, SeekOrigin.Begin);
        WriteSettings(
            writer,
            printer,
            layerCount,
            layerHeightMm,
            layerTableOffset,
            machineNameOffset,
            (uint)machineNameBytes.Length,
            disclaimerOffset,
            320);

        writer.BaseStream.Seek(0, SeekOrigin.Begin);
        WriteHeader(writer, (uint)settingsOffset, signatureOffset, 32);

        writer.BaseStream.Seek(fileLength, SeekOrigin.Begin);
        writer.Flush();
        return ms.ToArray();
    }

    internal static byte[] EncodeLayerRle(ReadOnlySpan<byte> pixels)
    {
        var rawData = new List<byte>(Math.Max(64, pixels.Length / 8));
        byte color = byte.MaxValue >> 1;
        uint stride = 0;

        void AddRun()
        {
            if (stride == 0)
                return;

            var code = color;
            if (stride > 1)
                code |= 0x80;

            rawData.Add(code);

            if (stride <= 1)
                return;

            if (stride <= 0x7F)
            {
                rawData.Add((byte)stride);
                return;
            }

            if (stride <= 0x3FFF)
            {
                rawData.Add((byte)((stride >> 8) | 0x80));
                rawData.Add((byte)stride);
                return;
            }

            if (stride <= 0x1FFFFF)
            {
                rawData.Add((byte)((stride >> 16) | 0xC0));
                rawData.Add((byte)(stride >> 8));
                rawData.Add((byte)stride);
                return;
            }

            rawData.Add((byte)((stride >> 24) | 0xE0));
            rawData.Add((byte)(stride >> 16));
            rawData.Add((byte)(stride >> 8));
            rawData.Add((byte)stride);
        }

        for (var i = 0; i < pixels.Length; i++)
        {
            var grey7 = (byte)(pixels[i] >> 1);
            if (grey7 == color)
            {
                stride++;
            }
            else
            {
                AddRun();
                color = grey7;
                stride = 1;
            }
        }

        AddRun();
        return rawData.ToArray();
    }

    internal static byte[] DecodeLayerRle(ReadOnlySpan<byte> data, int pixelCount)
    {
        var output = new byte[pixelCount];
        var pixel = 0;

        for (var n = 0; n < data.Length; n++)
        {
            var code = data[n];
            var stride = 1;

            if ((code & 0x80) == 0x80)
            {
                code &= 0x7F;
                n++;
                if (n >= data.Length)
                    throw new InvalidDataException("Corrupted CTB RLE stream.");

                var slen = data[n];
                if ((slen & 0x80) == 0)
                {
                    stride = slen;
                }
                else if ((slen & 0xC0) == 0x80)
                {
                    if (n + 1 >= data.Length)
                        throw new InvalidDataException("Corrupted CTB RLE stream.");
                    stride = ((slen & 0x3F) << 8) + data[n + 1];
                    n += 1;
                }
                else if ((slen & 0xE0) == 0xC0)
                {
                    if (n + 2 >= data.Length)
                        throw new InvalidDataException("Corrupted CTB RLE stream.");
                    stride = ((slen & 0x1F) << 16) + (data[n + 1] << 8) + data[n + 2];
                    n += 2;
                }
                else if ((slen & 0xF0) == 0xE0)
                {
                    if (n + 3 >= data.Length)
                        throw new InvalidDataException("Corrupted CTB RLE stream.");
                    stride = ((slen & 0x0F) << 24) + (data[n + 1] << 16) + (data[n + 2] << 8) + data[n + 3];
                    n += 3;
                }
                else
                {
                    throw new InvalidDataException("Corrupted CTB RLE stream.");
                }
            }

            var value = code == 0 ? (byte)0 : (byte)((code << 1) | 1);
            if (pixel + stride > output.Length)
                throw new InvalidDataException("CTB RLE stream overflows destination buffer.");

            output.AsSpan(pixel, stride).Fill(value);
            pixel += stride;
        }

        if (pixel != output.Length)
            throw new InvalidDataException("CTB RLE stream did not fill destination buffer.");

        return output;
    }

    private static void WriteHeaderPlaceholder(BinaryWriter writer)
    {
        writer.Write(new byte[HeaderSize]);
    }

    private static void WriteHeader(BinaryWriter writer, uint settingsOffset, uint signatureOffset, uint signatureSize)
    {
        writer.Write(Magic);
        writer.Write(SettingsSize);
        writer.Write(settingsOffset);
        writer.Write(0u);
        writer.Write(Version);
        writer.Write(signatureSize);
        writer.Write(signatureOffset);
        writer.Write(0u);
        writer.Write((ushort)1);
        writer.Write((ushort)1);
        writer.Write(0u);
        writer.Write(42u);
        writer.Write(0u);
    }

    private static void WriteLayerDefinition(BinaryWriter writer, int layerIndex, uint dataOffset, int dataLength, PrinterConfigDto printer)
    {
        var isBottom = layerIndex < printer.BottomLayerCount;
        var exposure = isBottom ? printer.BottomExposureTimeSeconds : printer.ExposureTimeSeconds;
        var lightOffDelay = isBottom ? printer.BottomLightOffDelaySeconds : printer.LightOffDelaySeconds;
        var liftHeight = isBottom ? printer.BottomLiftHeightMm : printer.LiftHeightMm;
        var liftSpeed = isBottom ? printer.BottomLiftSpeedMmPerMinute : printer.LiftSpeedMmPerMinute;
        var lightPwm = isBottom ? printer.BottomLightPwm : printer.LightPwm;

        writer.Write(LayerDefinitionSize);
        writer.Write((layerIndex + 1) * printer.LayerHeightMm);
        writer.Write(exposure);
        writer.Write(lightOffDelay);
        writer.Write(dataOffset);
        writer.Write(0u);
        writer.Write((uint)dataLength);
        writer.Write(0u);
        writer.Write(0u);
        writer.Write(0u);
        writer.Write(liftHeight);
        writer.Write(liftSpeed);
        writer.Write(0f);
        writer.Write(0f);
        writer.Write(printer.RetractSpeedMmPerMinute);
        writer.Write(0f);
        writer.Write(printer.RetractSpeedMmPerMinute);
        writer.Write(printer.WaitTimeAfterCureSeconds);
        writer.Write(printer.WaitTimeAfterLiftSeconds);
        writer.Write(printer.WaitTimeBeforeCureSeconds);
        writer.Write((float)lightPwm);
        writer.Write(0u);
    }

    private static void WriteSettings(
        BinaryWriter writer,
        PrinterConfigDto printer,
        int layerCount,
        float layerHeightMm,
        uint layerTableOffset,
        uint machineNameOffset,
        uint machineNameSize,
        uint disclaimerOffset,
        uint disclaimerSize)
    {
        var settingsStart = writer.BaseStream.Position;

        writer.Write((ulong)DateTimeOffset.UtcNow.ToUnixTimeSeconds());
        writer.Write(layerTableOffset);
        writer.Write(printer.BedWidthMm);
        writer.Write(printer.BedDepthMm);
        writer.Write(220f);
        writer.Write(0u);
        writer.Write(0u);
        writer.Write(layerCount * layerHeightMm);
        writer.Write(layerHeightMm);
        writer.Write(printer.ExposureTimeSeconds);
        writer.Write(printer.BottomExposureTimeSeconds);
        writer.Write(printer.LightOffDelaySeconds);
        writer.Write((uint)printer.BottomLayerCount);
        writer.Write((uint)printer.PixelWidth);
        writer.Write((uint)printer.PixelHeight);
        writer.Write((uint)layerCount);
        writer.Write(0u);
        writer.Write(0u);
        writer.Write(0u);
        writer.Write(1u);
        writer.Write(printer.BottomLiftHeightMm);
        writer.Write(printer.BottomLiftSpeedMmPerMinute);
        writer.Write(printer.LiftHeightMm);
        writer.Write(printer.LiftSpeedMmPerMinute);
        writer.Write(printer.RetractSpeedMmPerMinute);
        writer.Write(0f);
        writer.Write(0f);
        writer.Write(0f);
        writer.Write(printer.BottomLightOffDelaySeconds);
        writer.Write(1u);
        writer.Write((ushort)printer.LightPwm);
        writer.Write((ushort)printer.BottomLightPwm);
        writer.Write(0u);
        writer.Write(printer.BottomLiftHeightMm);
        writer.Write(printer.BottomLiftSpeedMmPerMinute);
        writer.Write(printer.LiftHeightMm);
        writer.Write(printer.LiftSpeedMmPerMinute);
        writer.Write(0f);
        writer.Write(printer.RetractSpeedMmPerMinute);
        writer.Write(printer.WaitTimeAfterLiftSeconds);
        writer.Write(machineNameOffset);
        writer.Write(machineNameSize);
        writer.Write((byte)0x0F);
        writer.Write((ushort)0);
        writer.Write((byte)0x40);
        writer.Write((uint)(DateTimeOffset.UtcNow.ToUnixTimeSeconds() / 60));
        writer.Write(8u);
        writer.Write(printer.WaitTimeBeforeCureSeconds);
        writer.Write(printer.WaitTimeAfterLiftSeconds);
        writer.Write((uint)printer.TransitionLayerCount);
        writer.Write(printer.RetractSpeedMmPerMinute);
        writer.Write(printer.RetractSpeedMmPerMinute);
        writer.Write(0u);
        writer.Write(4f);
        writer.Write(0u);
        writer.Write(4f);
        writer.Write(printer.WaitTimeBeforeCureSeconds);
        writer.Write(printer.WaitTimeAfterLiftSeconds);
        writer.Write(printer.WaitTimeAfterCureSeconds);
        writer.Write(0f);
        writer.Write(0u);
        writer.Write(0u);
        writer.Write(4u);
        writer.Write((uint)Math.Max(0, layerCount - 1));
        writer.Write(0u);
        writer.Write(0u);
        writer.Write(0u);
        writer.Write(0u);
        writer.Write(disclaimerOffset);
        writer.Write(disclaimerSize);
        writer.Write(0u);
        writer.Write(0u);
        writer.Write(0u);
        writer.Write(0u);

        var bytesWritten = writer.BaseStream.Position - settingsStart;
        if (bytesWritten > SettingsSize)
            throw new InvalidOperationException($"CTB settings block overflow: {bytesWritten} > {SettingsSize}");

        if (bytesWritten < SettingsSize)
            writer.Write(new byte[SettingsSize - bytesWritten]);
    }
}
