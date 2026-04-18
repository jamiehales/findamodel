namespace findamodel.Services;

public sealed class SliceBitmap
{
    private const int MinDiagonalSupportRunLength = 4;

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

    public void RemoveUnsupportedHorizontalPixels()
    {
        if (Width < 2 || Height < 2)
            return;

        var original = (byte[])Pixels.Clone();
        for (var y = 0; y < Height; y++)
        {
            var x = 0;
            while (x < Width)
            {
                var index = (y * Width) + x;
                if (original[index] == 0)
                {
                    x++;
                    continue;
                }

                var runStart = x;
                while (x + 1 < Width && original[(y * Width) + x + 1] > 0)
                    x++;

                var runEnd = x;
                if (!HasSufficientVerticalSupport(original, y, runStart, runEnd))
                {
                    for (var clearX = runStart; clearX <= runEnd; clearX++)
                        Pixels[(y * Width) + clearX] = 0;
                }

                x++;
            }
        }

        ClearUnsupportedRunInteriors(original);
        RepairVerticalDropouts(maxGapHeight: 2);
        FillSmallInteriorVoids();
        RepairThinInteriorHorizontalGaps();
        RemoveDetachedArtifacts();
    }

    private void RepairVerticalDropouts(int maxGapHeight)
    {
        if (Height < 3 || maxGapHeight < 1)
            return;

        for (var gapHeight = 1; gapHeight <= maxGapHeight; gapHeight++)
        {
            var current = (byte[])Pixels.Clone();
            for (var y = 1; y + gapHeight < Height; y++)
            {
                var topRow = y - 1;
                var bottomRow = y + gapHeight;
                if (bottomRow >= Height)
                    break;

                var x = 0;
                while (x < Width)
                {
                    if (!IsVerticalGapColumn(current, x, topRow, y, gapHeight, bottomRow))
                    {
                        x++;
                        continue;
                    }

                    var runStart = x;
                    while (x + 1 < Width && IsVerticalGapColumn(current, x + 1, topRow, y, gapHeight, bottomRow))
                        x++;

                    var runEnd = x;
                    for (var row = y; row < y + gapHeight; row++)
                    {
                        for (var fillX = runStart; fillX <= runEnd; fillX++)
                            Pixels[(row * Width) + fillX] = byte.MaxValue;
                    }

                    x++;
                }
            }
        }
    }

    private bool IsVerticalGapColumn(byte[] pixels, int x, int topRow, int gapStartRow, int gapHeight, int bottomRow)
    {
        if (pixels[(topRow * Width) + x] == 0 || pixels[(bottomRow * Width) + x] == 0)
            return false;

        for (var row = gapStartRow; row < gapStartRow + gapHeight; row++)
        {
            if (pixels[(row * Width) + x] > 0)
                return false;
        }

        return true;
    }

    private void FillSmallInteriorVoids()
    {
        if (Width < 3 || Height < 3)
            return;

        var visited = new bool[Pixels.Length];
        var stack = new Stack<int>();
        var component = new List<int>();

        for (var index = 0; index < Pixels.Length; index++)
        {
            if (Pixels[index] > 0 || visited[index])
                continue;

            component.Clear();
            stack.Push(index);
            visited[index] = true;

            var minX = Width;
            var minY = Height;
            var maxX = -1;
            var maxY = -1;
            var touchesBoundary = false;

            while (stack.Count > 0)
            {
                var current = stack.Pop();
                component.Add(current);

                var x = current % Width;
                var y = current / Width;
                minX = Math.Min(minX, x);
                maxX = Math.Max(maxX, x);
                minY = Math.Min(minY, y);
                maxY = Math.Max(maxY, y);
                touchesBoundary |= x == 0 || x == Width - 1 || y == 0 || y == Height - 1;

                for (var ny = Math.Max(0, y - 1); ny <= Math.Min(Height - 1, y + 1); ny++)
                {
                    for (var nx = Math.Max(0, x - 1); nx <= Math.Min(Width - 1, x + 1); nx++)
                    {
                        var neighborIndex = (ny * Width) + nx;
                        if (visited[neighborIndex] || Pixels[neighborIndex] > 0)
                            continue;

                        visited[neighborIndex] = true;
                        stack.Push(neighborIndex);
                    }
                }
            }

            var holeWidth = maxX - minX + 1;
            var holeHeight = maxY - minY + 1;
            var shouldFill = !touchesBoundary
                && holeWidth <= 96
                && holeHeight <= 32
                && component.Count <= 1600;

            if (!shouldFill)
                continue;

            foreach (var pixelIndex in component)
                Pixels[pixelIndex] = byte.MaxValue;
        }
    }

    private void RepairThinInteriorHorizontalGaps()
    {
        if (Width < 3 || Height < 3)
            return;

        for (var pass = 0; pass < 2; pass++)
        {
            var current = (byte[])Pixels.Clone();
            for (var y = 1; y < Height - 1; y++)
            {
                var x = 1;
                while (x < Width - 1)
                {
                    if (current[(y * Width) + x] > 0)
                    {
                        x++;
                        continue;
                    }

                    var runStart = x;
                    while (x + 1 < Width - 1 && current[(y * Width) + x + 1] == 0)
                        x++;

                    var runEnd = x;
                    if (current[(y * Width) + runStart - 1] > 0
                        && current[(y * Width) + runEnd + 1] > 0
                        && HasStrongVerticalCoverage(current, y, runStart, runEnd))
                    {
                        for (var fillX = runStart; fillX <= runEnd; fillX++)
                            Pixels[(y * Width) + fillX] = byte.MaxValue;
                    }

                    x++;
                }
            }
        }
    }

    private void ClearUnsupportedRunInteriors(byte[] referencePixels)
    {
        const int minUnsupportedGap = 4;

        for (var y = 0; y < Height; y++)
        {
            var x = 0;
            while (x < Width)
            {
                if (Pixels[(y * Width) + x] == 0)
                {
                    x++;
                    continue;
                }

                var runStart = x;
                while (x + 1 < Width && Pixels[(y * Width) + x + 1] > 0)
                    x++;

                var runEnd = x;
                if (runEnd - runStart + 1 <= minUnsupportedGap * 2)
                {
                    x++;
                    continue;
                }

                var gapStart = -1;
                for (var px = runStart; px <= runEnd; px++)
                {
                    var supported = false;
                    if (y > 0 && referencePixels[((y - 1) * Width) + px] > 0)
                        supported = true;

                    if (!supported && y + 1 < Height && referencePixels[((y + 1) * Width) + px] > 0)
                        supported = true;

                    if (!supported)
                    {
                        if (gapStart < 0)
                            gapStart = px;
                    }
                    else
                    {
                        if (gapStart >= 0 && px - gapStart >= minUnsupportedGap)
                        {
                            for (var cx = gapStart; cx < px; cx++)
                                Pixels[(y * Width) + cx] = 0;
                        }

                        gapStart = -1;
                    }
                }

                if (gapStart >= 0 && runEnd - gapStart + 1 >= minUnsupportedGap)
                {
                    for (var cx = gapStart; cx <= runEnd; cx++)
                        Pixels[(y * Width) + cx] = 0;
                }

                x++;
            }
        }
    }

    private void RemoveDetachedArtifacts()
    {
        if (Width < 2 || Height < 2)
            return;

        var visited = new bool[Pixels.Length];
        List<ComponentInfo>? components = null;

        for (var index = 0; index < Pixels.Length; index++)
        {
            if (Pixels[index] == 0 || visited[index])
                continue;

            components ??= [];
            components.Add(CollectComponent(index, visited));
        }

        if (components is null || components.Count <= 1)
            return;

        var largest = components.MaxBy(static component => component.Area);
        if (largest is null || largest.Area <= 0)
            return;

        foreach (var component in components)
        {
            if (ReferenceEquals(component, largest))
                continue;

            var width = component.MaxX - component.MinX + 1;
            var height = component.MaxY - component.MinY + 1;
            var dx = GapBetween(component.MinX, component.MaxX, largest.MinX, largest.MaxX);
            var dy = GapBetween(component.MinY, component.MaxY, largest.MinY, largest.MaxY);
            var boundsArea = width * height;
            var density = boundsArea > 0 ? (float)component.Area / boundsArea : 0f;
            var isClearlySmallerThanMain = component.Area * 3 <= largest.Area;
            var isThinHorizontal = height <= 2 && width >= 4;
            var isLongThinHorizontal = height <= 3 && width >= 24;
            var isSmallArtifactCluster = component.Area <= 24 && width <= 16 && height <= 16 && density <= 0.75f;
            var isSparseSlashCluster = component.Area <= 48 && width <= 20 && height <= 20 && density <= 0.55f;
            var isDetachedFromBody = dx > 0 || dy > 0;
            if (isClearlySmallerThanMain
                && isDetachedFromBody
                && dx <= 64
                && dy <= 8
                && (isThinHorizontal || isLongThinHorizontal || isSmallArtifactCluster || isSparseSlashCluster))
            {
                foreach (var pixelIndex in component.PixelIndexes)
                    Pixels[pixelIndex] = 0;
            }
        }
    }

    private ComponentInfo CollectComponent(int startIndex, bool[] visited)
    {
        var stack = new Stack<int>();
        var pixels = new List<int>();
        var minX = Width;
        var minY = Height;
        var maxX = -1;
        var maxY = -1;

        stack.Push(startIndex);
        visited[startIndex] = true;

        while (stack.Count > 0)
        {
            var index = stack.Pop();
            pixels.Add(index);

            var x = index % Width;
            var y = index / Width;
            minX = Math.Min(minX, x);
            maxX = Math.Max(maxX, x);
            minY = Math.Min(minY, y);
            maxY = Math.Max(maxY, y);

            for (var ny = Math.Max(0, y - 1); ny <= Math.Min(Height - 1, y + 1); ny++)
            {
                for (var nx = Math.Max(0, x - 1); nx <= Math.Min(Width - 1, x + 1); nx++)
                {
                    var neighborIndex = (ny * Width) + nx;
                    if (visited[neighborIndex] || Pixels[neighborIndex] == 0)
                        continue;

                    visited[neighborIndex] = true;
                    stack.Push(neighborIndex);
                }
            }
        }

        return new ComponentInfo(pixels, minX, maxX, minY, maxY);
    }

    private static int GapBetween(int minA, int maxA, int minB, int maxB)
    {
        if (maxA < minB)
            return minB - maxA - 1;
        if (maxB < minA)
            return minA - maxB - 1;
        return 0;
    }

    private bool HasStrongVerticalCoverage(byte[] pixels, int y, int runStart, int runEnd)
    {
        var length = runEnd - runStart + 1;
        if (length <= 0)
            return false;

        var aboveCoverage = 0;
        var belowCoverage = 0;
        for (var x = runStart; x <= runEnd; x++)
        {
            if (pixels[((y - 1) * Width) + x] > 0)
                aboveCoverage++;
            if (pixels[((y + 1) * Width) + x] > 0)
                belowCoverage++;
        }

        var requiredCoverage = Math.Max(1, (int)MathF.Ceiling(length * 0.6f));
        return aboveCoverage >= requiredCoverage && belowCoverage >= requiredCoverage;
    }

    private bool HasSufficientVerticalSupport(byte[] pixels, int y, int runStart, int runEnd)
    {
        if (HasExactVerticalSupport(pixels, y, runStart, runEnd))
            return true;

        var runLength = runEnd - runStart + 1;
        if (runLength < MinDiagonalSupportRunLength)
            return false;

        var aboveSupport = CountNearbyRowSupport(pixels, y - 1, runStart, runEnd);
        var belowSupport = CountNearbyRowSupport(pixels, y + 1, runStart, runEnd);
        return aboveSupport >= 2 && belowSupport >= 2;
    }

    private bool HasExactVerticalSupport(byte[] pixels, int y, int runStart, int runEnd)
    {
        if (y > 0)
        {
            for (var x = runStart; x <= runEnd; x++)
            {
                if (pixels[((y - 1) * Width) + x] > 0)
                    return true;
            }
        }

        if (y + 1 < Height)
        {
            for (var x = runStart; x <= runEnd; x++)
            {
                if (pixels[((y + 1) * Width) + x] > 0)
                    return true;
            }
        }

        return false;
    }

    private int CountNearbyRowSupport(byte[] pixels, int row, int runStart, int runEnd)
    {
        if (row < 0 || row >= Height)
            return 0;

        var count = 0;
        var minX = Math.Max(0, runStart - 2);
        var maxX = Math.Min(Width - 1, runEnd + 2);
        for (var x = minX; x <= maxX; x++)
        {
            if (pixels[(row * Width) + x] > 0)
                count++;
        }

        return count;
    }

    private sealed record ComponentInfo(List<int> PixelIndexes, int MinX, int MaxX, int MinY, int MaxY)
    {
        public int Area => PixelIndexes.Count;
    }
}
