using System.Diagnostics;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Threading.Channels;
using Microsoft.Extensions.Logging;
using Silk.NET.Maths;
using Silk.NET.OpenGL;
using Silk.NET.Windowing;
using Silk.NET.Windowing.Sdl;

namespace findamodel.Services;

public sealed class GlSliceProjectionContext : IDisposable
{
    private const int MaxGpuTriangleCount = 250_000;
    private const int DefaultRowGroupHeight = 8;
    private const int NvidiaRowGroupHeight = 16;
    private const int DefaultGridColumnCount = 16;
    private const int NvidiaGridColumnCount = 28;
    private const int GenericComputeWorkgroupSize = 8;
    private const int NvidiaComputeWorkgroupSize = 16;

    private const string VertSrc = """
        #version 330 core
        layout(location = 0) in vec2 aPos;
        void main() {
            gl_Position = vec4(aPos, 0.0, 1.0);
        }
        """;

    private const string FragSrc = """
        #version 330 core
        uniform sampler2D uTriangleTexture;
        uniform sampler2D uBoundsTexture;
        uniform isampler2D uIndexTexture;
        uniform sampler2D uRangeTexture;
        uniform int uTriangleTexWidth;
        uniform int uBoundsTexWidth;
        uniform int uIndexTexWidth;
        uniform int uRangeTexWidth;
        uniform int uRowGroupHeight;
        uniform int uRowGroupCount;
        uniform int uGridColumnCount;
        uniform float uSliceHeight;
        uniform float uBedWidth;
        uniform float uBedDepth;
        uniform float uRayOffset;
        uniform int uResolutionX;
        uniform int uResolutionY;

        out vec4 FragColor;

        const float kEpsilon = 0.0001;
        const float kDedupEpsilon = 0.0005;
        const int kMaxHits = 32;

        vec3 fetchVertex(int flatIndex) {
            int tx = flatIndex % uTriangleTexWidth;
            int ty = flatIndex / uTriangleTexWidth;
            return texelFetch(uTriangleTexture, ivec2(tx, ty), 0).xyz;
        }

        vec4 fetchBounds(int triIndex) {
            int tx = triIndex % uBoundsTexWidth;
            int ty = triIndex / uBoundsTexWidth;
            return texelFetch(uBoundsTexture, ivec2(tx, ty), 0);
        }

        int fetchTriangleIndex(int flatIndex) {
            int tx = flatIndex % uIndexTexWidth;
            int ty = flatIndex / uIndexTexWidth;
            return texelFetch(uIndexTexture, ivec2(tx, ty), 0).x;
        }

        ivec2 fetchRange(int rowGroup) {
            int tx = rowGroup % uRangeTexWidth;
            int ty = rowGroup / uRangeTexWidth;
            vec4 encoded = texelFetch(uRangeTexture, ivec2(tx, ty), 0);
            return ivec2(int(encoded.x + 0.5), int(encoded.y + 0.5));
        }

        bool tryIntersectPositiveXRay(vec3 origin, vec3 v0, vec3 v1, vec3 v2, out float hitX) {
            float maxX = max(v0.x, max(v1.x, v2.x));
            if (maxX < origin.x + kEpsilon) {
                hitX = 0.0;
                return false;
            }

            float minZ = min(v0.z, min(v1.z, v2.z));
            float maxZ = max(v0.z, max(v1.z, v2.z));
            if (origin.z < minZ - kEpsilon || origin.z > maxZ + kEpsilon) {
                hitX = 0.0;
                return false;
            }

            vec3 direction = vec3(1.0, 0.0, 0.0);
            vec3 edge1 = v1 - v0;
            vec3 edge2 = v2 - v0;
            vec3 h = cross(direction, edge2);
            float a = dot(edge1, h);
            if (abs(a) < kEpsilon) {
                hitX = 0.0;
                return false;
            }

            float invA = 1.0 / a;
            vec3 s = origin - v0;
            float u = invA * dot(s, h);
            if (u < -kEpsilon || u > 1.0 + kEpsilon) {
                hitX = 0.0;
                return false;
            }

            vec3 q = cross(s, edge1);
            float v = invA * dot(direction, q);
            if (v < -kEpsilon || u + v > 1.0 + kEpsilon) {
                hitX = 0.0;
                return false;
            }

            float t = invA * dot(edge2, q);
            if (t < kEpsilon) {
                hitX = 0.0;
                return false;
            }

            hitX = origin.x + t;
            return true;
        }

        void main() {
            float xMm = ((gl_FragCoord.x) / float(uResolutionX)) * uBedWidth - (uBedWidth * 0.5);
            float zMm = (((gl_FragCoord.y) / float(uResolutionY)) * uBedDepth) - (uBedDepth * 0.5);
            vec3 origin = vec3(xMm - uRayOffset, uSliceHeight, zMm);

            int rowFromTop = (uResolutionY - 1) - int(gl_FragCoord.y - 0.5);
            int rowGroup = clamp(rowFromTop / uRowGroupHeight, 0, max(0, uRowGroupCount - 1));
            int xGroup = clamp(int((gl_FragCoord.x - 1.0) / float(uResolutionX) * float(uGridColumnCount)), 0, max(0, uGridColumnCount - 1));
            ivec2 range = fetchRange((rowGroup * uGridColumnCount) + xGroup);

            float hits[kMaxHits];
            int hitDeltas[kMaxHits];
            int uniqueHitCount = 0;

            for (int offset = 0; offset < range.y; offset++) {
                int triIndex = fetchTriangleIndex(range.x + offset);
                vec4 bounds = fetchBounds(triIndex);

                if (uSliceHeight < bounds.x - kEpsilon || uSliceHeight > bounds.y + kEpsilon) {
                    continue;
                }

                if (origin.z < bounds.z - kEpsilon || origin.z > bounds.w + kEpsilon) {
                    continue;
                }

                vec3 v0 = fetchVertex(triIndex * 3);
                vec3 v1 = fetchVertex(triIndex * 3 + 1);
                vec3 v2 = fetchVertex(triIndex * 3 + 2);

                float hitX;
                if (!tryIntersectPositiveXRay(origin, v0, v1, v2, hitX)) {
                    continue;
                }

                int hitDelta = cross(v1 - v0, v2 - v0).x < 0.0 ? 1 : -1;
                bool accumulated = false;
                for (int hitIndex = 0; hitIndex < uniqueHitCount; hitIndex++) {
                    if (abs(hits[hitIndex] - hitX) <= kDedupEpsilon) {
                        hitDeltas[hitIndex] += hitDelta;
                        accumulated = true;
                        break;
                    }
                }

                if (!accumulated && uniqueHitCount < kMaxHits) {
                    hits[uniqueHitCount] = hitX;
                    hitDeltas[uniqueHitCount] = hitDelta;
                    uniqueHitCount++;
                }
            }

            int winding = 0;
            for (int hitIndex = 0; hitIndex < uniqueHitCount; hitIndex++) {
                winding += hitDeltas[hitIndex];
            }

            float value = winding != 0 ? 1.0 : 0.0;
            FragColor = vec4(value, value, value, 1.0);
        }
        """;

    private string BuildComputeShaderSource() => $$"""
        #version 430 core
        layout(local_size_x = {{computeWorkgroupSize}}, local_size_y = {{computeWorkgroupSize}}, local_size_z = 1) in;

        layout(r8, binding = 0) uniform writeonly image2D uOutput;
        uniform sampler2D uTriangleTexture;
        uniform sampler2D uBoundsTexture;
        uniform isampler2D uIndexTexture;
        uniform sampler2D uRangeTexture;
        uniform int uTriangleTexWidth;
        uniform int uBoundsTexWidth;
        uniform int uIndexTexWidth;
        uniform int uRangeTexWidth;
        uniform int uRowGroupHeight;
        uniform int uRowGroupCount;
        uniform int uGridColumnCount;
        uniform float uSliceHeight;
        uniform float uBedWidth;
        uniform float uBedDepth;
        uniform float uRayOffset;
        uniform int uResolutionX;
        uniform int uResolutionY;

        const float kEpsilon = 0.0001;
        const float kDedupEpsilon = 0.0005;
        const int kMaxHits = 32;

        vec3 fetchVertex(int flatIndex) {
            int tx = flatIndex % uTriangleTexWidth;
            int ty = flatIndex / uTriangleTexWidth;
            return texelFetch(uTriangleTexture, ivec2(tx, ty), 0).xyz;
        }

        vec4 fetchBounds(int triIndex) {
            int tx = triIndex % uBoundsTexWidth;
            int ty = triIndex / uBoundsTexWidth;
            return texelFetch(uBoundsTexture, ivec2(tx, ty), 0);
        }

        int fetchTriangleIndex(int flatIndex) {
            int tx = flatIndex % uIndexTexWidth;
            int ty = flatIndex / uIndexTexWidth;
            return texelFetch(uIndexTexture, ivec2(tx, ty), 0).x;
        }

        ivec2 fetchRange(int cellIndex) {
            int tx = cellIndex % uRangeTexWidth;
            int ty = cellIndex / uRangeTexWidth;
            vec4 encoded = texelFetch(uRangeTexture, ivec2(tx, ty), 0);
            return ivec2(int(encoded.x + 0.5), int(encoded.y + 0.5));
        }

        bool tryIntersectPositiveXRay(vec3 origin, vec3 v0, vec3 v1, vec3 v2, out float hitX) {
            float maxX = max(v0.x, max(v1.x, v2.x));
            if (maxX < origin.x + kEpsilon) {
                hitX = 0.0;
                return false;
            }

            float minZ = min(v0.z, min(v1.z, v2.z));
            float maxZ = max(v0.z, max(v1.z, v2.z));
            if (origin.z < minZ - kEpsilon || origin.z > maxZ + kEpsilon) {
                hitX = 0.0;
                return false;
            }

            vec3 direction = vec3(1.0, 0.0, 0.0);
            vec3 edge1 = v1 - v0;
            vec3 edge2 = v2 - v0;
            vec3 h = cross(direction, edge2);
            float a = dot(edge1, h);
            if (abs(a) < kEpsilon) {
                hitX = 0.0;
                return false;
            }

            float invA = 1.0 / a;
            vec3 s = origin - v0;
            float u = invA * dot(s, h);
            if (u < -kEpsilon || u > 1.0 + kEpsilon) {
                hitX = 0.0;
                return false;
            }

            vec3 q = cross(s, edge1);
            float v = invA * dot(direction, q);
            if (v < -kEpsilon || u + v > 1.0 + kEpsilon) {
                hitX = 0.0;
                return false;
            }

            float t = invA * dot(edge2, q);
            if (t < kEpsilon) {
                hitX = 0.0;
                return false;
            }

            hitX = origin.x + t;
            return true;
        }

        void main() {
            ivec2 pixel = ivec2(gl_GlobalInvocationID.xy);
            if (pixel.x >= uResolutionX || pixel.y >= uResolutionY) {
                return;
            }

            float xMm = ((float(pixel.x) + 0.5) / float(uResolutionX)) * uBedWidth - (uBedWidth * 0.5);
            float zMm = ((((float(pixel.y) + 0.5) / float(uResolutionY)) * uBedDepth) - (uBedDepth * 0.5));
            vec3 origin = vec3(xMm - uRayOffset, uSliceHeight, zMm);

            int rowFromTop = (uResolutionY - 1) - pixel.y;
            int rowGroup = clamp(rowFromTop / uRowGroupHeight, 0, max(0, uRowGroupCount - 1));
            int xGroup = clamp((pixel.x * uGridColumnCount) / max(1, uResolutionX), 0, max(0, uGridColumnCount - 1));
            ivec2 range = fetchRange((rowGroup * uGridColumnCount) + xGroup);

            float hits[kMaxHits];
            int hitDeltas[kMaxHits];
            int uniqueHitCount = 0;

            for (int offset = 0; offset < range.y; offset++) {
                int triIndex = fetchTriangleIndex(range.x + offset);
                vec4 bounds = fetchBounds(triIndex);

                if (uSliceHeight < bounds.x - kEpsilon || uSliceHeight > bounds.y + kEpsilon) {
                    continue;
                }

                if (origin.z < bounds.z - kEpsilon || origin.z > bounds.w + kEpsilon) {
                    continue;
                }

                vec3 v0 = fetchVertex(triIndex * 3);
                vec3 v1 = fetchVertex(triIndex * 3 + 1);
                vec3 v2 = fetchVertex(triIndex * 3 + 2);

                float hitX;
                if (!tryIntersectPositiveXRay(origin, v0, v1, v2, hitX)) {
                    continue;
                }

                int hitDelta = cross(v1 - v0, v2 - v0).x < 0.0 ? 1 : -1;
                bool accumulated = false;
                for (int hitIndex = 0; hitIndex < uniqueHitCount; hitIndex++) {
                    if (abs(hits[hitIndex] - hitX) <= kDedupEpsilon) {
                        hitDeltas[hitIndex] += hitDelta;
                        accumulated = true;
                        break;
                    }
                }

                if (!accumulated && uniqueHitCount < kMaxHits) {
                    hits[uniqueHitCount] = hitX;
                    hitDeltas[uniqueHitCount] = hitDelta;
                    uniqueHitCount++;
                }
            }

            int winding = 0;
            for (int hitIndex = 0; hitIndex < uniqueHitCount; hitIndex++) {
                winding += hitDeltas[hitIndex];
            }

            float value = winding != 0 ? 1.0 : 0.0;
            imageStore(uOutput, pixel, vec4(value, 0.0, 0.0, 1.0));
        }
        """;

    private readonly ILogger logger;
    private readonly Channel<SliceRequest> channel = Channel.CreateBounded<SliceRequest>(new BoundedChannelOptions(16)
    {
        FullMode = BoundedChannelFullMode.Wait,
    });
    private readonly Thread glThread;
    private readonly TaskCompletionSource ready = new(TaskCreationOptions.RunContinuationsAsynchronously);

    private GL? gl;
    private bool failed;
    private string rendererName = "unknown";
    private string renderBackend = "fragment";
    private bool useNvidiaFastPath;
    private bool supportsComputeShaders;
    private int activeRowGroupHeight = DefaultRowGroupHeight;
    private int activeGridColumnCount = DefaultGridColumnCount;
    private int computeWorkgroupSize = GenericComputeWorkgroupSize;

    private uint program;
    private uint computeProgram;
    private uint vao;
    private uint vbo;
    private uint fbo;
    private uint colorTexture;
    private uint triangleTexture;
    private uint boundsTexture;
    private uint indexTexture;
    private uint rangeTexture;

    private int fboWidth;
    private int fboHeight;
    private int maxTextureSize = 4096;
    private int cachedTriangleTextureWidth;
    private int cachedBoundsTextureWidth;
    private int cachedIndexTextureWidth;
    private int cachedRangeTextureWidth;
    private int cachedRowGroupCount;
    private ulong cachedGeometryHash;
    private int cachedGeometryTriangleCount = -1;
    private int cachedSpatialPixelHeight = -1;
    private float cachedSpatialBedWidthMm = float.NaN;
    private float cachedSpatialBedDepthMm = float.NaN;

    private int uTriangleTextureLocation;
    private int uBoundsTextureLocation;
    private int uIndexTextureLocation;
    private int uRangeTextureLocation;
    private int uTriangleTexWidthLocation;
    private int uBoundsTexWidthLocation;
    private int uIndexTexWidthLocation;
    private int uRangeTexWidthLocation;
    private int uRowGroupHeightLocation;
    private int uRowGroupCountLocation;
    private int uGridColumnCountLocation;
    private int uSliceHeightLocation;
    private int uBedWidthLocation;
    private int uBedDepthLocation;
    private int uRayOffsetLocation;
    private int uResolutionXLocation;
    private int uResolutionYLocation;

    public GlSliceProjectionContext(ILoggerFactory loggerFactory)
    {
        logger = loggerFactory.CreateLogger<GlSliceProjectionContext>();
        glThread = new Thread(GlThreadMain)
        {
            IsBackground = true,
            Name = "GL-SliceProjection",
        };
        glThread.Start();
    }

    public bool IsAvailable
    {
        get
        {
            ready.Task.GetAwaiter().GetResult();
            return !failed;
        }
    }

    public Task WaitReadyAsync() => ready.Task;

    public string ActiveBackend => renderBackend;

    public SliceBitmap? TryRender(
        IReadOnlyList<Triangle3D> triangles,
        float sliceHeightMm,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight)
    {
        var result = TryRenderBatch(triangles, [sliceHeightMm], bedWidthMm, bedDepthMm, pixelWidth, pixelHeight);
        return result is { Count: > 0 } ? result[0] : null;
    }

    public IReadOnlyList<SliceBitmap>? TryRenderBatch(
        IReadOnlyList<Triangle3D> triangles,
        IReadOnlyList<float> sliceHeightsMm,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight)
    {
        ready.Task.GetAwaiter().GetResult();
        if (failed || triangles.Count == 0 || triangles.Count > MaxGpuTriangleCount || sliceHeightsMm.Count == 0)
            return null;

        var tcs = new TaskCompletionSource<IReadOnlyList<SliceBitmap>?>(TaskCreationOptions.RunContinuationsAsynchronously);
        var request = new SliceRequest(
            tcs,
            triangles,
            sliceHeightsMm.ToArray(),
            bedWidthMm,
            bedDepthMm,
            pixelWidth,
            pixelHeight);

        channel.Writer.WriteAsync(request).AsTask().GetAwaiter().GetResult();
        return tcs.Task.GetAwaiter().GetResult();
    }

    public void Dispose()
    {
        channel.Writer.TryComplete();
        glThread.Join(5000);
    }

    private static IWindow CreateHiddenWindow(int major, int minor)
    {
        var opts = WindowOptions.Default;
        opts.IsVisible = false;
        opts.Size = new Vector2D<int>(1, 1);
        opts.API = new GraphicsAPI(ContextAPI.OpenGL, ContextProfile.Core, ContextFlags.Default, new APIVersion(major, minor));
        opts.VSync = false;
        return Window.Create(opts);
    }

    private void GlThreadMain()
    {
        IWindow? window = null;

        try
        {
            if (RuntimeInformation.IsOSPlatform(OSPlatform.Linux))
                Environment.SetEnvironmentVariable("SDL_VIDEODRIVER", "offscreen");

            SdlWindowing.Use();

            try
            {
                window = CreateHiddenWindow(4, 3);
                window.Initialize();
                gl = window.CreateOpenGL();
            }
            catch
            {
                window?.Dispose();
                window = CreateHiddenWindow(3, 3);
                window.Initialize();
                gl = window.CreateOpenGL();
            }

            gl.GetInteger(GLEnum.MaxTextureSize, out maxTextureSize);
            gl.PixelStore(PixelStoreParameter.PackAlignment, 1);
            gl.PixelStore(PixelStoreParameter.UnpackAlignment, 1);

            rendererName = gl.GetStringS(StringName.Renderer) ?? "unknown";
            useNvidiaFastPath = rendererName.Contains("NVIDIA", StringComparison.OrdinalIgnoreCase);
            activeRowGroupHeight = useNvidiaFastPath ? NvidiaRowGroupHeight : DefaultRowGroupHeight;
            activeGridColumnCount = useNvidiaFastPath ? NvidiaGridColumnCount : DefaultGridColumnCount;
            computeWorkgroupSize = useNvidiaFastPath ? NvidiaComputeWorkgroupSize : GenericComputeWorkgroupSize;

            CompileShaders();
            SetupQuad();
            logger.LogInformation("GL slice projection ready on {Renderer} using backend {Backend}.", rendererName, renderBackend);
            ready.TrySetResult();
        }
        catch (Exception ex)
        {
            failed = true;
            logger.LogDebug(ex, "GL slice projection context unavailable.");
            ready.TrySetResult();
            window?.Dispose();
            return;
        }

        while (channel.Reader.WaitToReadAsync().AsTask().GetAwaiter().GetResult())
        {
            while (channel.Reader.TryRead(out var request))
            {
                try
                {
                    var bitmaps = RenderInternal(request);
                    request.Completion.TrySetResult(bitmaps);
                }
                catch (Exception ex)
                {
                    logger.LogDebug(ex, "GL slice projection render failed.");
                    request.Completion.TrySetResult(null);
                }
            }
        }

        if (gl is not null)
        {
            if (fbo != 0)
                gl.DeleteFramebuffer(fbo);
            if (colorTexture != 0)
                gl.DeleteTexture(colorTexture);
            if (triangleTexture != 0)
                gl.DeleteTexture(triangleTexture);
            if (boundsTexture != 0)
                gl.DeleteTexture(boundsTexture);
            if (indexTexture != 0)
                gl.DeleteTexture(indexTexture);
            if (rangeTexture != 0)
                gl.DeleteTexture(rangeTexture);
            if (program != 0)
                gl.DeleteProgram(program);
            if (computeProgram != 0)
                gl.DeleteProgram(computeProgram);
            if (vao != 0)
                gl.DeleteVertexArray(vao);
            if (vbo != 0)
                gl.DeleteBuffer(vbo);
            gl.Dispose();
        }

        window?.Dispose();
    }

    private IReadOnlyList<SliceBitmap>? RenderInternal(SliceRequest request)
    {
        if (gl is null)
            return null;

        EnsureFramebuffer(request.PixelWidth, request.PixelHeight);

        var uploadSw = Stopwatch.StartNew();
        EnsureGeometryResourcesUploaded(request.Triangles, request.BedWidthMm, request.BedDepthMm, request.PixelHeight);
        uploadSw.Stop();

        if (cachedTriangleTextureWidth <= 0 || cachedBoundsTextureWidth <= 0 || cachedIndexTextureWidth <= 0 || cachedRangeTextureWidth <= 0)
            return null;

        gl.BindFramebuffer(FramebufferTarget.Framebuffer, fbo);
        gl.Viewport(0, 0, (uint)request.PixelWidth, (uint)request.PixelHeight);

        var renderSw = Stopwatch.StartNew();
        var results = supportsComputeShaders
            ? RenderWithCompute(request)
            : RenderWithFragment(request);
        renderSw.Stop();

        gl.BindFramebuffer(FramebufferTarget.Framebuffer, 0);

        logger.LogDebug(
            "GL slice batch on {Renderer} using {Backend}: triangles={TriangleCount} layers={LayerCount} uploadMs={UploadMs:F2} renderMs={RenderMs:F2}",
            rendererName,
            renderBackend,
            request.Triangles.Count,
            request.SliceHeightsMm.Length,
            uploadSw.Elapsed.TotalMilliseconds,
            renderSw.Elapsed.TotalMilliseconds);

        return results;
    }

    private IReadOnlyList<SliceBitmap> RenderWithFragment(SliceRequest request)
    {
        var results = new List<SliceBitmap>(request.SliceHeightsMm.Length);

        gl!.UseProgram(program);
        gl.BindVertexArray(vao);

        gl.ActiveTexture(TextureUnit.Texture0);
        gl.BindTexture(TextureTarget.Texture2D, triangleTexture);
        gl.Uniform1(uTriangleTextureLocation, 0);

        gl.ActiveTexture(TextureUnit.Texture1);
        gl.BindTexture(TextureTarget.Texture2D, boundsTexture);
        gl.Uniform1(uBoundsTextureLocation, 1);

        gl.ActiveTexture(TextureUnit.Texture2);
        gl.BindTexture(TextureTarget.Texture2D, indexTexture);
        gl.Uniform1(uIndexTextureLocation, 2);

        gl.ActiveTexture(TextureUnit.Texture3);
        gl.BindTexture(TextureTarget.Texture2D, rangeTexture);
        gl.Uniform1(uRangeTextureLocation, 3);

        gl.Uniform1(uTriangleTexWidthLocation, cachedTriangleTextureWidth);
        gl.Uniform1(uBoundsTexWidthLocation, cachedBoundsTextureWidth);
        gl.Uniform1(uIndexTexWidthLocation, cachedIndexTextureWidth);
        gl.Uniform1(uRangeTexWidthLocation, cachedRangeTextureWidth);
        gl.Uniform1(uRowGroupHeightLocation, activeRowGroupHeight);
        gl.Uniform1(uRowGroupCountLocation, cachedRowGroupCount);
        gl.Uniform1(uGridColumnCountLocation, activeGridColumnCount);
        gl.Uniform1(uBedWidthLocation, request.BedWidthMm);
        gl.Uniform1(uBedDepthLocation, request.BedDepthMm);
        gl.Uniform1(uRayOffsetLocation, 0.0005f);
        gl.Uniform1(uResolutionXLocation, request.PixelWidth);
        gl.Uniform1(uResolutionYLocation, request.PixelHeight);

        foreach (var sliceHeightMm in request.SliceHeightsMm)
        {
            gl.ClearColor(0f, 0f, 0f, 1f);
            gl.Clear(ClearBufferMask.ColorBufferBit);
            gl.Uniform1(uSliceHeightLocation, sliceHeightMm);
            gl.DrawArrays(PrimitiveType.Triangles, 0, 6);

            var rawPixels = new byte[request.PixelWidth * request.PixelHeight];
            gl.ReadPixels(0, 0, (uint)request.PixelWidth, (uint)request.PixelHeight, PixelFormat.Red, PixelType.UnsignedByte, rawPixels.AsSpan());
            results.Add(FlipToBitmap(rawPixels, request.PixelWidth, request.PixelHeight));
        }

        return results;
    }

    private IReadOnlyList<SliceBitmap> RenderWithCompute(SliceRequest request)
    {
        var gl = this.gl!;
        var results = new List<SliceBitmap>(request.SliceHeightsMm.Length);

        gl.UseProgram(computeProgram);
        gl.ActiveTexture(TextureUnit.Texture0);
        gl.BindTexture(TextureTarget.Texture2D, triangleTexture);
        gl.Uniform1(gl.GetUniformLocation(computeProgram, "uTriangleTexture"), 0);

        gl.ActiveTexture(TextureUnit.Texture1);
        gl.BindTexture(TextureTarget.Texture2D, boundsTexture);
        gl.Uniform1(gl.GetUniformLocation(computeProgram, "uBoundsTexture"), 1);

        gl.ActiveTexture(TextureUnit.Texture2);
        gl.BindTexture(TextureTarget.Texture2D, indexTexture);
        gl.Uniform1(gl.GetUniformLocation(computeProgram, "uIndexTexture"), 2);

        gl.ActiveTexture(TextureUnit.Texture3);
        gl.BindTexture(TextureTarget.Texture2D, rangeTexture);
        gl.Uniform1(gl.GetUniformLocation(computeProgram, "uRangeTexture"), 3);

        gl.Uniform1(gl.GetUniformLocation(computeProgram, "uTriangleTexWidth"), cachedTriangleTextureWidth);
        gl.Uniform1(gl.GetUniformLocation(computeProgram, "uBoundsTexWidth"), cachedBoundsTextureWidth);
        gl.Uniform1(gl.GetUniformLocation(computeProgram, "uIndexTexWidth"), cachedIndexTextureWidth);
        gl.Uniform1(gl.GetUniformLocation(computeProgram, "uRangeTexWidth"), cachedRangeTextureWidth);
        gl.Uniform1(gl.GetUniformLocation(computeProgram, "uRowGroupHeight"), activeRowGroupHeight);
        gl.Uniform1(gl.GetUniformLocation(computeProgram, "uRowGroupCount"), cachedRowGroupCount);
        gl.Uniform1(gl.GetUniformLocation(computeProgram, "uGridColumnCount"), activeGridColumnCount);
        gl.Uniform1(gl.GetUniformLocation(computeProgram, "uBedWidth"), request.BedWidthMm);
        gl.Uniform1(gl.GetUniformLocation(computeProgram, "uBedDepth"), request.BedDepthMm);
        gl.Uniform1(gl.GetUniformLocation(computeProgram, "uRayOffset"), 0.0005f);
        gl.Uniform1(gl.GetUniformLocation(computeProgram, "uResolutionX"), request.PixelWidth);
        gl.Uniform1(gl.GetUniformLocation(computeProgram, "uResolutionY"), request.PixelHeight);

        var groupsX = (uint)((request.PixelWidth + computeWorkgroupSize - 1) / computeWorkgroupSize);
        var groupsY = (uint)((request.PixelHeight + computeWorkgroupSize - 1) / computeWorkgroupSize);

        foreach (var sliceHeightMm in request.SliceHeightsMm)
        {
            gl.Uniform1(gl.GetUniformLocation(computeProgram, "uSliceHeight"), sliceHeightMm);
            gl.BindImageTexture(0, colorTexture, 0, false, 0, GLEnum.WriteOnly, GLEnum.R8);
            gl.DispatchCompute(groupsX, groupsY, 1);
            gl.MemoryBarrier(MemoryBarrierMask.ShaderImageAccessBarrierBit | MemoryBarrierMask.TextureFetchBarrierBit | MemoryBarrierMask.FramebufferBarrierBit);

            var rawPixels = new byte[request.PixelWidth * request.PixelHeight];
            gl.ReadPixels(0, 0, (uint)request.PixelWidth, (uint)request.PixelHeight, PixelFormat.Red, PixelType.UnsignedByte, rawPixels.AsSpan());
            results.Add(FlipToBitmap(rawPixels, request.PixelWidth, request.PixelHeight));
        }

        return results;
    }

    private void EnsureGeometryResourcesUploaded(IReadOnlyList<Triangle3D> triangles, float bedWidthMm, float bedDepthMm, int pixelHeight)
    {
        if (gl is null)
            return;

        var geometryHash = ComputeGeometryHash(triangles);
        var geometryChanged = geometryHash != cachedGeometryHash || triangles.Count != cachedGeometryTriangleCount;
        var spatialChanged = geometryChanged
            || pixelHeight != cachedSpatialPixelHeight
            || MathF.Abs(bedWidthMm - cachedSpatialBedWidthMm) > 0.0001f
            || MathF.Abs(bedDepthMm - cachedSpatialBedDepthMm) > 0.0001f;

        if (geometryChanged)
        {
            cachedTriangleTextureWidth = UploadTriangleTexture(triangles);
            cachedBoundsTextureWidth = UploadBoundsTexture(triangles);
            cachedGeometryHash = geometryHash;
            cachedGeometryTriangleCount = triangles.Count;
        }

        if (spatialChanged)
        {
            var (indexWidth, rangeWidth, rowGroupCount) = UploadSpatialIndexTextures(triangles, bedWidthMm, bedDepthMm, pixelHeight);
            cachedIndexTextureWidth = indexWidth;
            cachedRangeTextureWidth = rangeWidth;
            cachedRowGroupCount = rowGroupCount;
            cachedSpatialPixelHeight = pixelHeight;
            cachedSpatialBedWidthMm = bedWidthMm;
            cachedSpatialBedDepthMm = bedDepthMm;
        }
    }

    private int UploadTriangleTexture(IReadOnlyList<Triangle3D> triangles)
    {
        if (gl is null)
            return 0;

        var totalTexels = triangles.Count * 3;
        if (totalTexels <= 0)
            return 0;

        var width = Math.Min(maxTextureSize, Math.Max(1, totalTexels));
        var height = (int)Math.Ceiling(totalTexels / (double)width);
        var data = new Half[width * height * 4];

        var offset = 0;
        for (var i = 0; i < triangles.Count; i++)
        {
            var triangle = triangles[i];
            WriteVertex(data, ref offset, triangle.V0);
            WriteVertex(data, ref offset, triangle.V1);
            WriteVertex(data, ref offset, triangle.V2);
        }

        if (triangleTexture == 0)
            triangleTexture = gl.GenTexture();

        gl.BindTexture(TextureTarget.Texture2D, triangleTexture);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureMinFilter, (int)GLEnum.Nearest);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureMagFilter, (int)GLEnum.Nearest);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureWrapS, (int)GLEnum.ClampToEdge);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureWrapT, (int)GLEnum.ClampToEdge);
        gl.TexImage2D(TextureTarget.Texture2D, 0, (int)InternalFormat.Rgba16f, (uint)width, (uint)height, 0, PixelFormat.Rgba, PixelType.HalfFloat, in data[0]);

        return width;
    }

    private int UploadBoundsTexture(IReadOnlyList<Triangle3D> triangles)
    {
        if (gl is null)
            return 0;

        var totalTexels = triangles.Count;
        if (totalTexels <= 0)
            return 0;

        var width = Math.Min(maxTextureSize, Math.Max(1, totalTexels));
        var height = (int)Math.Ceiling(totalTexels / (double)width);
        var data = new Half[width * height * 4];

        var offset = 0;
        for (var i = 0; i < triangles.Count; i++)
        {
            var triangle = triangles[i];
            var minY = MathF.Min(triangle.V0.Y, MathF.Min(triangle.V1.Y, triangle.V2.Y));
            var maxY = MathF.Max(triangle.V0.Y, MathF.Max(triangle.V1.Y, triangle.V2.Y));
            var minZ = MathF.Min(triangle.V0.Z, MathF.Min(triangle.V1.Z, triangle.V2.Z));
            var maxZ = MathF.Max(triangle.V0.Z, MathF.Max(triangle.V1.Z, triangle.V2.Z));

            data[offset++] = (Half)minY;
            data[offset++] = (Half)maxY;
            data[offset++] = (Half)minZ;
            data[offset++] = (Half)maxZ;
        }

        if (boundsTexture == 0)
            boundsTexture = gl.GenTexture();

        gl.BindTexture(TextureTarget.Texture2D, boundsTexture);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureMinFilter, (int)GLEnum.Nearest);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureMagFilter, (int)GLEnum.Nearest);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureWrapS, (int)GLEnum.ClampToEdge);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureWrapT, (int)GLEnum.ClampToEdge);
        gl.TexImage2D(TextureTarget.Texture2D, 0, (int)InternalFormat.Rgba16f, (uint)width, (uint)height, 0, PixelFormat.Rgba, PixelType.HalfFloat, in data[0]);

        return width;
    }

    private (int IndexWidth, int RangeWidth, int RowGroupCount) UploadSpatialIndexTextures(IReadOnlyList<Triangle3D> triangles, float bedWidthMm, float bedDepthMm, int pixelHeight)
    {
        if (gl is null)
            return (0, 0, 0);

        var localRowGroupHeight = useNvidiaFastPath ? NvidiaRowGroupHeight : DefaultRowGroupHeight;
        var maxTexels = Math.Max(1, maxTextureSize * maxTextureSize);
        while (localRowGroupHeight < pixelHeight)
        {
            var estimatedGroups = (pixelHeight + localRowGroupHeight - 1) / localRowGroupHeight;
            if ((long)estimatedGroups * Math.Max(1, triangles.Count) * activeGridColumnCount <= maxTexels)
                break;

            localRowGroupHeight *= 2;
        }

        activeRowGroupHeight = Math.Max(1, localRowGroupHeight);
        var rowGroupCount = Math.Max(1, (pixelHeight + activeRowGroupHeight - 1) / activeRowGroupHeight);
        var cellCount = rowGroupCount * activeGridColumnCount;
        var groups = new List<int>[cellCount];

        for (var index = 0; index < triangles.Count; index++)
        {
            var triangle = triangles[index];
            var minZ = MathF.Min(triangle.V0.Z, MathF.Min(triangle.V1.Z, triangle.V2.Z));
            var maxZ = MathF.Max(triangle.V0.Z, MathF.Max(triangle.V1.Z, triangle.V2.Z));
            var maxX = MathF.Max(triangle.V0.X, MathF.Max(triangle.V1.X, triangle.V2.X));

            var startRow = Math.Clamp(MapZToRow(maxZ, bedDepthMm, pixelHeight), 0, pixelHeight - 1);
            var endRow = Math.Clamp(MapZToRow(minZ, bedDepthMm, pixelHeight), 0, pixelHeight - 1);
            if (endRow < startRow)
                (startRow, endRow) = (endRow, startRow);

            var startGroup = Math.Clamp(startRow / activeRowGroupHeight, 0, rowGroupCount - 1);
            var endGroup = Math.Clamp(endRow / activeRowGroupHeight, 0, rowGroupCount - 1);
            var xEndGroup = Math.Clamp((int)MathF.Floor(((maxX + (bedWidthMm * 0.5f)) / bedWidthMm) * activeGridColumnCount), 0, activeGridColumnCount - 1);

            for (var groupIndex = startGroup; groupIndex <= endGroup; groupIndex++)
            {
                for (var xGroup = 0; xGroup <= xEndGroup; xGroup++)
                {
                    var cellIndex = (groupIndex * activeGridColumnCount) + xGroup;
                    groups[cellIndex] ??= [];
                    groups[cellIndex].Add(index);
                }
            }
        }

        var flatIndexes = new List<int>(Math.Max(triangles.Count, cellCount));
        var ranges = new int[Math.Max(1, cellCount * 2)];
        for (var cellIndex = 0; cellIndex < cellCount; cellIndex++)
        {
            ranges[cellIndex * 2] = flatIndexes.Count;
            var candidates = groups[cellIndex];
            if (candidates is not null)
                flatIndexes.AddRange(candidates);
            ranges[(cellIndex * 2) + 1] = candidates?.Count ?? 0;
        }

        if (flatIndexes.Count == 0)
            flatIndexes.Add(0);

        var indexWidth = Math.Min(maxTextureSize, Math.Max(1, flatIndexes.Count));
        var indexHeight = (int)Math.Ceiling(flatIndexes.Count / (double)indexWidth);
        var indexData = new int[indexWidth * indexHeight];
        for (var i = 0; i < flatIndexes.Count; i++)
            indexData[i] = flatIndexes[i];

        if (indexTexture == 0)
            indexTexture = gl.GenTexture();

        gl.BindTexture(TextureTarget.Texture2D, indexTexture);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureMinFilter, (int)GLEnum.Nearest);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureMagFilter, (int)GLEnum.Nearest);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureWrapS, (int)GLEnum.ClampToEdge);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureWrapT, (int)GLEnum.ClampToEdge);
        gl.TexImage2D(TextureTarget.Texture2D, 0, (int)InternalFormat.R32i, (uint)indexWidth, (uint)indexHeight, 0, PixelFormat.RedInteger, PixelType.Int, in indexData[0]);

        var rangeWidth = Math.Min(maxTextureSize, Math.Max(1, cellCount));
        var rangeHeight = (int)Math.Ceiling(cellCount / (double)rangeWidth);
        var rangeData = new float[Math.Max(1, rangeWidth * rangeHeight * 4)];
        for (var i = 0; i < cellCount; i++)
        {
            rangeData[i * 4] = ranges[i * 2];
            rangeData[(i * 4) + 1] = ranges[(i * 2) + 1];
        }

        if (rangeTexture == 0)
            rangeTexture = gl.GenTexture();

        gl.BindTexture(TextureTarget.Texture2D, rangeTexture);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureMinFilter, (int)GLEnum.Nearest);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureMagFilter, (int)GLEnum.Nearest);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureWrapS, (int)GLEnum.ClampToEdge);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureWrapT, (int)GLEnum.ClampToEdge);
        gl.TexImage2D(TextureTarget.Texture2D, 0, (int)InternalFormat.Rgba32f, (uint)rangeWidth, (uint)rangeHeight, 0, PixelFormat.Rgba, PixelType.Float, in rangeData[0]);

        return (indexWidth, rangeWidth, rowGroupCount);
    }

    private void EnsureFramebuffer(int width, int height)
    {
        if (gl is null)
            return;

        if (fbo != 0 && fboWidth == width && fboHeight == height)
            return;

        if (fbo != 0)
        {
            gl.DeleteFramebuffer(fbo);
            gl.DeleteTexture(colorTexture);
            fbo = 0;
            colorTexture = 0;
        }

        fboWidth = width;
        fboHeight = height;

        fbo = gl.GenFramebuffer();
        gl.BindFramebuffer(FramebufferTarget.Framebuffer, fbo);

        colorTexture = gl.GenTexture();
        gl.BindTexture(TextureTarget.Texture2D, colorTexture);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureMinFilter, (int)GLEnum.Nearest);
        gl.TexParameter(TextureTarget.Texture2D, TextureParameterName.TextureMagFilter, (int)GLEnum.Nearest);
        var emptyTexture = new byte[width * height];
        gl.TexImage2D(TextureTarget.Texture2D, 0, (int)InternalFormat.R8, (uint)width, (uint)height, 0, PixelFormat.Red, PixelType.UnsignedByte, in emptyTexture[0]);
        gl.FramebufferTexture2D(FramebufferTarget.Framebuffer, FramebufferAttachment.ColorAttachment0, TextureTarget.Texture2D, colorTexture, 0);

        gl.BindFramebuffer(FramebufferTarget.Framebuffer, 0);
    }

    private void SetupQuad()
    {
        if (gl is null)
            return;

        float[] vertices =
        [
            -1f, -1f,
             1f, -1f,
            -1f,  1f,
            -1f,  1f,
             1f, -1f,
             1f,  1f,
        ];

        vao = gl.GenVertexArray();
        vbo = gl.GenBuffer();

        gl.BindVertexArray(vao);
        gl.BindBuffer(BufferTargetARB.ArrayBuffer, vbo);
        gl.BufferData<float>(BufferTargetARB.ArrayBuffer, vertices.AsSpan(), BufferUsageARB.StaticDraw);
        gl.EnableVertexAttribArray(0);
        gl.VertexAttribPointer(0, 2, VertexAttribPointerType.Float, false, 2 * sizeof(float), 0);
    }

    private void CompileShaders()
    {
        if (gl is null)
            return;

        var vert = gl.CreateShader(ShaderType.VertexShader);
        gl.ShaderSource(vert, VertSrc);
        gl.CompileShader(vert);
        CheckShader(vert, "vertex");

        var frag = gl.CreateShader(ShaderType.FragmentShader);
        gl.ShaderSource(frag, FragSrc);
        gl.CompileShader(frag);
        CheckShader(frag, "fragment");

        program = gl.CreateProgram();
        gl.AttachShader(program, vert);
        gl.AttachShader(program, frag);
        gl.LinkProgram(program);
        CheckProgram(program);

        gl.DeleteShader(vert);
        gl.DeleteShader(frag);

        uTriangleTextureLocation = gl.GetUniformLocation(program, "uTriangleTexture");
        uBoundsTextureLocation = gl.GetUniformLocation(program, "uBoundsTexture");
        uIndexTextureLocation = gl.GetUniformLocation(program, "uIndexTexture");
        uRangeTextureLocation = gl.GetUniformLocation(program, "uRangeTexture");
        uTriangleTexWidthLocation = gl.GetUniformLocation(program, "uTriangleTexWidth");
        uBoundsTexWidthLocation = gl.GetUniformLocation(program, "uBoundsTexWidth");
        uIndexTexWidthLocation = gl.GetUniformLocation(program, "uIndexTexWidth");
        uRangeTexWidthLocation = gl.GetUniformLocation(program, "uRangeTexWidth");
        uRowGroupHeightLocation = gl.GetUniformLocation(program, "uRowGroupHeight");
        uRowGroupCountLocation = gl.GetUniformLocation(program, "uRowGroupCount");
        uGridColumnCountLocation = gl.GetUniformLocation(program, "uGridColumnCount");
        uSliceHeightLocation = gl.GetUniformLocation(program, "uSliceHeight");
        uBedWidthLocation = gl.GetUniformLocation(program, "uBedWidth");
        uBedDepthLocation = gl.GetUniformLocation(program, "uBedDepth");
        uRayOffsetLocation = gl.GetUniformLocation(program, "uRayOffset");
        uResolutionXLocation = gl.GetUniformLocation(program, "uResolutionX");
        uResolutionYLocation = gl.GetUniformLocation(program, "uResolutionY");

        supportsComputeShaders = false;
        renderBackend = useNvidiaFastPath ? "nvidia-fragment" : "fragment";

        try
        {
            var compute = gl.CreateShader(ShaderType.ComputeShader);
            gl.ShaderSource(compute, BuildComputeShaderSource());
            gl.CompileShader(compute);
            CheckShader(compute, "compute");

            computeProgram = gl.CreateProgram();
            gl.AttachShader(computeProgram, compute);
            gl.LinkProgram(computeProgram);
            CheckProgram(computeProgram);
            gl.DeleteShader(compute);

            supportsComputeShaders = true;
            renderBackend = useNvidiaFastPath ? "nvidia-compute" : "compute";
        }
        catch (Exception ex)
        {
            if (computeProgram != 0)
            {
                gl.DeleteProgram(computeProgram);
                computeProgram = 0;
            }

            logger.LogDebug(ex, "Compute shader slice backend unavailable; using fragment fallback.");
        }
    }

    private void CheckShader(uint shader, string stage)
    {
        if (gl is null)
            return;

        gl.GetShader(shader, ShaderParameterName.CompileStatus, out var ok);
        if (ok == 0)
            throw new InvalidOperationException($"Slice projection {stage} shader compile failed: {gl.GetShaderInfoLog(shader)}");
    }

    private void CheckProgram(uint linkedProgram)
    {
        if (gl is null)
            return;

        gl.GetProgram(linkedProgram, ProgramPropertyARB.LinkStatus, out var ok);
        if (ok == 0)
            throw new InvalidOperationException($"Slice projection program link failed: {gl.GetProgramInfoLog(linkedProgram)}");
    }

    private static SliceBitmap FlipToBitmap(byte[] rawPixels, int width, int height)
    {
        var bitmap = new SliceBitmap(width, height);
        for (var row = 0; row < height; row++)
        {
            var sourceRow = height - 1 - row;
            Array.Copy(rawPixels, sourceRow * width, bitmap.Pixels, row * width, width);
        }

        return bitmap;
    }

    private static ulong ComputeGeometryHash(IReadOnlyList<Triangle3D> triangles)
    {
        const ulong offset = 14695981039346656037UL;
        const ulong prime = 1099511628211UL;
        var hash = offset;

        unchecked
        {
            hash ^= (uint)RuntimeHelpers.GetHashCode(triangles);
            hash *= prime;
            hash ^= (uint)triangles.Count;
            hash *= prime;
        }

        if (triangles.Count == 0)
            return hash;

        var sampleIndexes = new[]
        {
            0,
            triangles.Count / 4,
            triangles.Count / 2,
            (triangles.Count * 3) / 4,
            triangles.Count - 1,
        };

        var lastIndex = -1;
        foreach (var sampleIndex in sampleIndexes)
        {
            var clampedIndex = Math.Clamp(sampleIndex, 0, triangles.Count - 1);
            if (clampedIndex == lastIndex)
                continue;

            lastIndex = clampedIndex;
            var triangle = triangles[clampedIndex];
            hash = HashFloat(hash, triangle.V0.X, prime);
            hash = HashFloat(hash, triangle.V0.Y, prime);
            hash = HashFloat(hash, triangle.V0.Z, prime);
            hash = HashFloat(hash, triangle.V1.X, prime);
            hash = HashFloat(hash, triangle.V1.Y, prime);
            hash = HashFloat(hash, triangle.V1.Z, prime);
            hash = HashFloat(hash, triangle.V2.X, prime);
            hash = HashFloat(hash, triangle.V2.Y, prime);
            hash = HashFloat(hash, triangle.V2.Z, prime);
        }

        return hash;
    }

    private static ulong HashFloat(ulong current, float value, ulong prime)
    {
        unchecked
        {
            current ^= (uint)BitConverter.SingleToInt32Bits(value);
            current *= prime;
            return current;
        }
    }

    private static void WriteVertex(Half[] data, ref int offset, Vec3 vertex)
    {
        data[offset++] = (Half)vertex.X;
        data[offset++] = (Half)vertex.Y;
        data[offset++] = (Half)vertex.Z;
        data[offset++] = (Half)1f;
    }

    private static int MapZToRow(float zMm, float bedDepthMm, int height)
        => (int)MathF.Floor((((bedDepthMm * 0.5f) - zMm) / bedDepthMm) * height);

    private sealed record SliceRequest(
        TaskCompletionSource<IReadOnlyList<SliceBitmap>?> Completion,
        IReadOnlyList<Triangle3D> Triangles,
        float[] SliceHeightsMm,
        float BedWidthMm,
        float BedDepthMm,
        int PixelWidth,
        int PixelHeight);
}
