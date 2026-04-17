namespace findamodel.Services;

public sealed class SliceBitmap
{
    public int Width { get; }
    public int Height { get; }
    public byte[] Pixels { get; }

    public SliceBitmap(int width, int height)
    {
        if (width <= 0 || height <= 0)
            throw new ArgumentException("Bitmap dimensions must be positive.");

        Width = width;
        Height = height;
        Pixels = new byte[checked(width * height)];
    }

    public byte GetPixel(int x, int y) => Pixels[(y * Width) + x];

    public void SetPixel(int x, int y, byte value) => Pixels[(y * Width) + x] = value;

    public Span<byte> GetRowSpan(int y) => Pixels.AsSpan(y * Width, Width);

    public int CountLitPixels()
    {
        var count = 0;
        foreach (var pixel in Pixels)
        {
            if (pixel > 0)
                count++;
        }

        return count;
    }
}
