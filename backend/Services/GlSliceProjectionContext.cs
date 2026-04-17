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
        uniform int uTriangleCount;
        uniform int uTriangleTexWidth;
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
            float zMm = (uBedDepth * 0.5) - ((gl_FragCoord.y) / float(uResolutionY)) * uBedDepth;
            vec3 origin = vec3(xMm - uRayOffset, uSliceHeight, zMm);

            float hits[kMaxHits];
            int uniqueHitCount = 0;

            for (int triIndex = 0; triIndex < uTriangleCount; triIndex++) {
                vec3 v0 = fetchVertex(triIndex * 3);
                vec3 v1 = fetchVertex(triIndex * 3 + 1);
                vec3 v2 = fetchVertex(triIndex * 3 + 2);

                float hitX;
                if (!tryIntersectPositiveXRay(origin, v0, v1, v2, hitX)) {
                    continue;
                }

                bool duplicate = false;
                for (int hitIndex = 0; hitIndex < uniqueHitCount; hitIndex++) {
                    if (abs(hits[hitIndex] - hitX) <= kDedupEpsilon) {
                        duplicate = true;
                        break;
                    }
                }

                if (!duplicate && uniqueHitCount < kMaxHits) {
                    hits[uniqueHitCount++] = hitX;
                }
            }

            float value = (uniqueHitCount % 2) == 1 ? 1.0 : 0.0;
            FragColor = vec4(value, value, value, 1.0);
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
    private uint program;
    private uint vao;
    private uint vbo;
    private uint fbo;
    private uint colorTexture;
    private uint triangleTexture;
    private int fboWidth;
    private int fboHeight;
    private int maxTextureSize = 4096;

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

    public SliceBitmap? TryRender(
        IReadOnlyList<Triangle3D> triangles,
        float sliceHeightMm,
        float bedWidthMm,
        float bedDepthMm,
        int pixelWidth,
        int pixelHeight)
    {
        ready.Task.GetAwaiter().GetResult();
        if (failed || triangles.Count == 0 || triangles.Count > MaxGpuTriangleCount)
            return null;

        var tcs = new TaskCompletionSource<SliceBitmap?>(TaskCreationOptions.RunContinuationsAsynchronously);
        channel.Writer.WriteAsync(new SliceRequest(
            tcs,
            triangles,
            sliceHeightMm,
            bedWidthMm,
            bedDepthMm,
            pixelWidth,
            pixelHeight)).AsTask().GetAwaiter().GetResult();

        return tcs.Task.GetAwaiter().GetResult();
    }

    public void Dispose()
    {
        channel.Writer.TryComplete();
        glThread.Join(5000);
    }

    private void GlThreadMain()
    {
        IWindow? window = null;

        try
        {
            if (RuntimeInformation.IsOSPlatform(OSPlatform.Linux))
                Environment.SetEnvironmentVariable("SDL_VIDEODRIVER", "offscreen");

            SdlWindowing.Use();

            var opts = WindowOptions.Default;
            opts.IsVisible = false;
            opts.Size = new Vector2D<int>(1, 1);
            opts.API = new GraphicsAPI(ContextAPI.OpenGL, ContextProfile.Core, ContextFlags.Default, new APIVersion(3, 3));
            opts.VSync = false;

            window = Window.Create(opts);
            window.Initialize();
            gl = window.CreateOpenGL();
            gl.GetInteger(GLEnum.MaxTextureSize, out maxTextureSize);

            CompileShaders();
            SetupQuad();
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
                    var bitmap = RenderInternal(request);
                    request.Completion.TrySetResult(bitmap);
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
            if (program != 0)
                gl.DeleteProgram(program);
            if (vao != 0)
                gl.DeleteVertexArray(vao);
            if (vbo != 0)
                gl.DeleteBuffer(vbo);
            gl.Dispose();
        }

        window?.Dispose();
    }

    private SliceBitmap? RenderInternal(SliceRequest request)
    {
        if (gl is null)
            return null;

        EnsureFramebuffer(request.PixelWidth, request.PixelHeight);
        var triangleTextureWidth = UploadTriangleTexture(request.Triangles);
        if (triangleTextureWidth <= 0)
            return null;

        gl.BindFramebuffer(FramebufferTarget.Framebuffer, fbo);
        gl.Viewport(0, 0, (uint)request.PixelWidth, (uint)request.PixelHeight);
        gl.ClearColor(0f, 0f, 0f, 1f);
        gl.Clear(ClearBufferMask.ColorBufferBit);

        gl.UseProgram(program);
        gl.BindVertexArray(vao);
        gl.ActiveTexture(TextureUnit.Texture0);
        gl.BindTexture(TextureTarget.Texture2D, triangleTexture);
        gl.Uniform1(gl.GetUniformLocation(program, "uTriangleTexture"), 0);
        gl.Uniform1(gl.GetUniformLocation(program, "uTriangleCount"), request.Triangles.Count);
        gl.Uniform1(gl.GetUniformLocation(program, "uTriangleTexWidth"), triangleTextureWidth);
        gl.Uniform1(gl.GetUniformLocation(program, "uSliceHeight"), request.SliceHeightMm);
        gl.Uniform1(gl.GetUniformLocation(program, "uBedWidth"), request.BedWidthMm);
        gl.Uniform1(gl.GetUniformLocation(program, "uBedDepth"), request.BedDepthMm);
        gl.Uniform1(gl.GetUniformLocation(program, "uRayOffset"), 0.0005f);
        gl.Uniform1(gl.GetUniformLocation(program, "uResolutionX"), request.PixelWidth);
        gl.Uniform1(gl.GetUniformLocation(program, "uResolutionY"), request.PixelHeight);
        gl.DrawArrays(PrimitiveType.Triangles, 0, 6);
        gl.Finish();

        byte[] rawPixels = new byte[request.PixelWidth * request.PixelHeight * 4];
        gl.ReadPixels(0, 0, (uint)request.PixelWidth, (uint)request.PixelHeight, PixelFormat.Rgba, PixelType.UnsignedByte, rawPixels.AsSpan());
        gl.BindFramebuffer(FramebufferTarget.Framebuffer, 0);

        var bitmap = new SliceBitmap(request.PixelWidth, request.PixelHeight);
        for (var row = 0; row < request.PixelHeight; row++)
        {
            var sourceRow = request.PixelHeight - 1 - row;
            for (var column = 0; column < request.PixelWidth; column++)
            {
                var sourceIndex = ((sourceRow * request.PixelWidth) + column) * 4;
                bitmap.SetPixel(column, row, rawPixels[sourceIndex]);
            }
        }

        return bitmap;
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
        var data = new float[width * height * 4];

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
        gl.TexImage2D(TextureTarget.Texture2D, 0, (int)InternalFormat.Rgba32f, (uint)width, (uint)height, 0, PixelFormat.Rgba, PixelType.Float, in data[0]);

        return width;
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
        var emptyTexture = new byte[width * height * 4];
        gl.TexImage2D(TextureTarget.Texture2D, 0, (int)InternalFormat.Rgba8, (uint)width, (uint)height, 0, PixelFormat.Rgba, PixelType.UnsignedByte, in emptyTexture[0]);
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

    private static void WriteVertex(float[] data, ref int offset, Vec3 vertex)
    {
        data[offset++] = vertex.X;
        data[offset++] = vertex.Y;
        data[offset++] = vertex.Z;
        data[offset++] = 1f;
    }

    private sealed record SliceRequest(
        TaskCompletionSource<SliceBitmap?> Completion,
        IReadOnlyList<Triangle3D> Triangles,
        float SliceHeightMm,
        float BedWidthMm,
        float BedDepthMm,
        int PixelWidth,
        int PixelHeight);
}
