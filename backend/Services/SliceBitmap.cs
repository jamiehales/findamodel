using System.Buffers;

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
        Parallel.For(0, Height, y =>
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
        });

        ClearUnsupportedRunInteriors(original);
        RepairVerticalDropouts(maxGapHeight: 2);
        FillSmallInteriorVoids();
        RepairThinInteriorHorizontalGaps();
        RemoveDetachedArtifacts();
    }

    internal void RepairVerticalDropouts(int maxGapHeight)
    {
        if (Height < 3 || maxGapHeight < 1)
            return;

        for (var gapHeight = 1; gapHeight <= maxGapHeight; gapHeight++)
        {
            var current = (byte[])Pixels.Clone();
            var gh = gapHeight;
            Parallel.For(1, Height - gh, y =>
            {
                var topRow = y - 1;
                var bottomRow = y + gh;
                if (bottomRow >= Height)
                    return;

                var x = 0;
                while (x < Width)
                {
                    if (!IsVerticalGapColumn(current, x, topRow, y, gh, bottomRow))
                    {
                        x++;
                        continue;
                    }

                    var runStart = x;
                    while (x + 1 < Width && IsVerticalGapColumn(current, x + 1, topRow, y, gh, bottomRow))
                        x++;

                    var runEnd = x;
                    for (var row = y; row < y + gh; row++)
                    {
                        for (var fillX = runStart; fillX <= runEnd; fillX++)
                            Pixels[(row * Width) + fillX] = byte.MaxValue;
                    }

                    x++;
                }
            });
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

    internal void FillSmallInteriorVoids()
    {
        if (Width < 3 || Height < 3)
            return;

        // Compute bounding box of lit pixels
        int litMinX = Width, litMinY = Height, litMaxX = -1, litMaxY = -1;
        for (var y = 0; y < Height; y++)
        {
            var rowOffset = y * Width;
            for (var x = 0; x < Width; x++)
            {
                if (Pixels[rowOffset + x] == 0) continue;
                if (x < litMinX) litMinX = x;
                if (x > litMaxX) litMaxX = x;
                if (y < litMinY) litMinY = y;
                if (y > litMaxY) litMaxY = y;
            }
        }

        if (litMaxX < 0)
            return;

        var bMinX = Math.Max(0, litMinX - 1);
        var bMinY = Math.Max(0, litMinY - 1);
        var bMaxX = Math.Min(Width - 1, litMaxX + 1);
        var bMaxY = Math.Min(Height - 1, litMaxY + 1);

        // Run-length connected component labeling (8-connectivity)
        // Collect empty pixel runs within bounding box
        var runs = new List<(int Row, int Start, int End)>();
        var rowRunStart = new int[bMaxY - bMinY + 2]; // index into runs list per row

        for (var y = bMinY; y <= bMaxY; y++)
        {
            rowRunStart[y - bMinY] = runs.Count;
            var rowOffset = y * Width;
            var x = bMinX;

            while (x <= bMaxX)
            {
                if (Pixels[rowOffset + x] > 0) { x++; continue; }
                var start = x;
                while (x <= bMaxX && Pixels[rowOffset + x] == 0) x++;
                runs.Add((y, start, x - 1));
            }
        }

        rowRunStart[bMaxY - bMinY + 1] = runs.Count;

        if (runs.Count == 0)
            return;

        // Union-find over runs
        var ufParent = new int[runs.Count];
        for (var i = 0; i < runs.Count; i++)
            ufParent[i] = i;

        // Connect runs on adjacent rows (8-connectivity: overlap if start <= other.end+1 && end >= other.start-1)
        for (var y = bMinY + 1; y <= bMaxY; y++)
        {
            var curStart = rowRunStart[y - bMinY];
            var curEnd = rowRunStart[y - bMinY + 1];
            var prevStart = rowRunStart[y - bMinY - 1];
            var prevEnd = rowRunStart[y - bMinY];

            var pi = prevStart;
            for (var ci = curStart; ci < curEnd; ci++)
            {
                var (_, cStart, cEnd) = runs[ci];

                // Advance prev pointer past runs that end before current starts (with 8-conn margin)
                while (pi < prevEnd && runs[pi].End < cStart - 1)
                    pi++;

                // Connect all overlapping runs on previous row
                for (var pj = pi; pj < prevEnd && runs[pj].Start <= cEnd + 1; pj++)
                    UfUnion(ufParent, ci, pj);
            }
        }

        // Resolve all labels and compute component stats
        var compMinX = new int[runs.Count];
        var compMinY = new int[runs.Count];
        var compMaxX = new int[runs.Count];
        var compMaxY = new int[runs.Count];
        var compArea = new int[runs.Count];
        var compBoundary = new bool[runs.Count];

        Array.Fill(compMinX, int.MaxValue);
        Array.Fill(compMinY, int.MaxValue);
        Array.Fill(compMaxX, -1);
        Array.Fill(compMaxY, -1);

        for (var i = 0; i < runs.Count; i++)
        {
            var root = UfFind(ufParent, i);
            var (row, start, end) = runs[i];
            var runLen = end - start + 1;

            compArea[root] += runLen;
            if (start < compMinX[root]) compMinX[root] = start;
            if (end > compMaxX[root]) compMaxX[root] = end;
            if (row < compMinY[root]) compMinY[root] = row;
            if (row > compMaxY[root]) compMaxY[root] = row;
            compBoundary[root] |= start == 0 || end == Width - 1
                || row == 0 || row == Height - 1
                || start <= bMinX || end >= bMaxX
                || row <= bMinY || row >= bMaxY;
        }

        // Fill qualifying voids
        for (var i = 0; i < runs.Count; i++)
        {
            var root = UfFind(ufParent, i);
            if (compBoundary[root]) continue;
            if (compArea[root] > 1600) continue;
            var w = compMaxX[root] - compMinX[root] + 1;
            var h = compMaxY[root] - compMinY[root] + 1;
            if (w > 96 || h > 32) continue;

            var (row, start, end) = runs[i];
            var rowOffset = row * Width;
            for (var px = start; px <= end; px++)
                Pixels[rowOffset + px] = byte.MaxValue;
        }
    }

    private static int UfFind(int[] parent, int x)
    {
        while (parent[x] != x)
        {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }

        return x;
    }

    private static void UfUnion(int[] parent, int a, int b)
    {
        var ra = UfFind(parent, a);
        var rb = UfFind(parent, b);
        if (ra != rb)
            parent[rb] = ra;
    }

    internal void RepairThinInteriorHorizontalGaps()
    {
        if (Width < 3 || Height < 3)
            return;

        for (var pass = 0; pass < 2; pass++)
        {
            var current = (byte[])Pixels.Clone();
            Parallel.For(1, Height - 1, y =>
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
            });
        }
    }

    internal void ClearUnsupportedRunInteriors(byte[] referencePixels)
    {
        const int minUnsupportedGap = 4;

        Parallel.For(0, Height, y =>
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
        });
    }

    internal void RemoveDetachedArtifacts()
    {
        if (Width < 2 || Height < 2)
            return;

        var visited = ArrayPool<bool>.Shared.Rent(Pixels.Length);
        Array.Clear(visited, 0, Pixels.Length);
        List<ComponentInfo>? components = null;

        try
        {
            for (var index = 0; index < Pixels.Length; index++)
            {
                if (Pixels[index] == 0 || visited[index])
                    continue;

                components ??= [];
                components.Add(CollectComponent(index, visited));
            }
        }
        finally
        {
            ArrayPool<bool>.Shared.Return(visited);
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
