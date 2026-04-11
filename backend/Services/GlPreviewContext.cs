using System.Numerics;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Threading.Channels;
using SixLabors.ImageSharp;
using SixLabors.ImageSharp.PixelFormats;
using Silk.NET.Maths;
using Silk.NET.OpenGL;
using Silk.NET.Windowing;
using Silk.NET.Windowing.Sdl;

namespace findamodel.Services;

/// <summary>
/// Hardware-accelerated (OpenGL 3.3 core) preview renderer.
/// Owns a single headless GL context shared across all renders (serialised via a
/// dedicated render thread). Falls back gracefully when OpenGL is unavailable.
///
/// Camera / coordinate system mirrors ModelViewer.tsx exactly:
///   - Y-up right-hand coordinate system (same as Three.js)
///   - Camera direction: normalize(1, 0.8, -1)  ← DEFAULT_VIEW_DIRECTION
///   - Vertical FOV: 45° (π/4 rad)              ← Canvas camera={{ fov: 45 }}
///   - Framing padding: 1.15×                   ← FRAMING_PADDING
///   - Background: #0f172a
///
/// Lighting mirrors the Three.js scene:
///   - ambientLight intensity 0.4
///   - directionalLight [5,8,5] intensity 1.2
///   - directionalLight [-4,2,-2] intensity 0.4
///   - directionalLight [0,-3,-5] intensity 0.25
/// </summary>
public sealed class GlPreviewContext : IDisposable
{
    private readonly ILogger _logger;

    // Single-item channel: render requests queued and executed on the GL thread.
    private readonly Channel<RenderRequest> _channel =
        Channel.CreateBounded<RenderRequest>(new BoundedChannelOptions(64)
        {
            FullMode = BoundedChannelFullMode.Wait,
        });

    private readonly Thread _glThread;
    private bool _failed;

    // Set once GL is ready (or failed) so callers don't queue work when context is broken.
    private readonly TaskCompletionSource _ready = new();

    // ── GL objects (all owned by _glThread) ──────────────────────────────────
    private GL? _gl;
    private uint _program;
    private uint _vao;
    private uint _vbo;

    // FBO set (rebuilt when render size changes)
    private uint _msaaFbo, _msaaColorRbo, _msaaDepthRbo;
    private uint _resolveFbo, _resolveColorRbo;
    private int _fboWidth, _fboHeight;

    // ── GLSL ─────────────────────────────────────────────────────────────────
    private const string VertSrc = """
        #version 330 core
        layout(location = 0) in vec3 aPos;
        layout(location = 1) in vec3 aNormal;

        uniform mat4 uModel;
        uniform mat4 uView;
        uniform mat4 uProj;

        out vec3 vNormal;
        out vec3 vFragPos;

        void main() {
            vec4 worldPos = uModel * vec4(aPos, 1.0);
            vFragPos = worldPos.xyz;
            // Model is identity so normal matrix is identity
            vNormal = aNormal;
            gl_Position = uProj * uView * worldPos;
        }
        """;

    private const string FragSrc = """
        #version 330 core

        in  vec3 vNormal;
        in  vec3 vFragPos;
        out vec4 FragColor;

        // Matches ModelViewer.tsx lighting:
        //   ambientLight    intensity={0.4}
        //   directionalLight position={[ 5,  8,  5]} intensity={1.2}
        //   directionalLight position={[-4,  2, -2]} intensity={0.4}
        //   directionalLight position={[ 0, -3, -5]} intensity={0.25}
        uniform vec3 uCameraPos;
        uniform vec3 uAlbedo;  // file-type accent colour (set per render)

        const float kAmbient   = 0.40;
        const float kDiffuse   = 0.70;
        const float kSpecular  = 0.15;
        const float kShininess = 20.0;

        struct DirLight { vec3 dir; float intensity; };
        const DirLight lights[3] = DirLight[3](
            DirLight(normalize(vec3( 5.0,  8.0,  5.0)), 1.20),
            DirLight(normalize(vec3(-4.0,  2.0, -2.0)), 0.40),
            DirLight(normalize(vec3( 0.0, -3.0, -5.0)), 0.25)
        );

        void main() {
            vec3 N = normalize(vNormal);
            vec3 V = normalize(uCameraPos - vFragPos);

            vec3 color = kAmbient * uAlbedo;

            for (int i = 0; i < 3; ++i) {
                float diff = max(dot(N, lights[i].dir), 0.0);
                color += kDiffuse * diff * lights[i].intensity * uAlbedo;

                vec3 R = reflect(-lights[i].dir, N);
                float spec = pow(max(dot(V, R), 0.0), kShininess);
                color += kSpecular * spec * lights[i].intensity;
            }

            FragColor = vec4(clamp(color, 0.0, 1.0), 1.0);
        }
        """;

    // ── Camera constants (mirror ModelViewer.tsx) ─────────────────────────────
    private const float FovY = MathF.PI / 4f;   // 45° — Canvas camera={{ fov: 45 }}
    private const float FramingPadding = 1.15f;           // FRAMING_PADDING in ModelViewer.tsx
    private static readonly System.Numerics.Vector3 CamDir =
        System.Numerics.Vector3.Normalize(new System.Numerics.Vector3(1f, 0.8f, -1f));

    // ── Public API ────────────────────────────────────────────────────────────

    public GlPreviewContext(ILoggerFactory loggerFactory)
    {
        _logger = loggerFactory.CreateLogger(LogChannels.Preview);
        _glThread = new Thread(GlThreadMain) { IsBackground = true, Name = "GL-Preview" };
        _glThread.Start();
    }

    /// <summary>
    /// True once the GL context initialised successfully.
    /// False if context creation failed (CPU fallback should be used).
    /// </summary>
    public bool IsAvailable => _ready.Task.IsCompletedSuccessfully && !_failed;

    /// <summary>
    /// Waits for GL initialisation to complete (success or failure).
    /// Must be awaited before the first <see cref="RenderAsync"/> call if you want
    /// to know whether GPU rendering is available.
    /// </summary>
    public Task WaitReadyAsync() => _ready.Task;

    /// <summary>
    /// Renders <paramref name="triangles"/> to a PNG and returns the bytes,
    /// or null if the GL context is unavailable.
    /// </summary>
    public async Task<byte[]?> RenderAsync(
        List<Triangle3D> triangles,
        int width, int height,
        System.Numerics.Vector3 albedo,
        List<Triangle3D>? supportTriangles = null,
        System.Numerics.Vector3 supportAlbedo = default)
    {
        await _ready.Task.ConfigureAwait(false);
        if (_failed) return null;

        var tcs = new TaskCompletionSource<byte[]?>(TaskCreationOptions.RunContinuationsAsynchronously);
        await _channel.Writer.WriteAsync(new RenderRequest(tcs, triangles, width, height, albedo, supportTriangles, supportAlbedo))
                             .ConfigureAwait(false);
        return await tcs.Task.ConfigureAwait(false);
    }

    public void Dispose()
    {
        _channel.Writer.TryComplete();
        _glThread.Join(5_000);
    }

    // ── GL thread ─────────────────────────────────────────────────────────────

    private void GlThreadMain()
    {
        IWindow? window = null;
        try
        {
            // On Linux / Docker, SDL offscreen driver creates EGL contexts without a display.
            if (RuntimeInformation.IsOSPlatform(OSPlatform.Linux))
                Environment.SetEnvironmentVariable("SDL_VIDEODRIVER", "offscreen");

            SdlWindowing.Use();

            var opts = WindowOptions.Default;
            opts.IsVisible = false;
            opts.Size = new Vector2D<int>(1, 1);
            opts.API = new GraphicsAPI(
                ContextAPI.OpenGL, ContextProfile.Core,
                ContextFlags.Default, new APIVersion(3, 3));
            // Disable vsync — we're rendering offscreen
            opts.VSync = false;

            window = Window.Create(opts);
            window.Initialize();

            _gl = window.CreateOpenGL();

            string renderer = _gl.GetStringS(StringName.Renderer) ?? "unknown";
            _logger.LogInformation("GL context ready. Renderer: {Renderer}. Version: {Version}",
                renderer, _gl.GetStringS(StringName.Version));

            CompileShaders();
            SetupVao();

            _ready.TrySetResult();
        }
        catch (Exception ex)
        {
            _failed = true;
            _logger.LogWarning(ex, "GL context creation failed — GPU rendering unavailable; falling back to CPU rasterizer");
            _ready.TrySetResult();   // signal "ready" so callers don't hang; IsAvailable = false
            DrainChannel();
            window?.Dispose();
            return;
        }

        // Process render requests until channel is closed
        while (_channel.Reader.WaitToReadAsync().AsTask().GetAwaiter().GetResult())
        {
            while (_channel.Reader.TryRead(out var req))
            {
                try
                {
                    byte[]? png = RenderInternal(req.Triangles, req.Width, req.Height, req.Albedo, req.SupportTriangles, req.SupportAlbedo);
                    req.Tcs.TrySetResult(png);
                }
                catch (Exception ex)
                {
                    _logger.LogError(ex, "GL render failed");
                    req.Tcs.TrySetResult(null);
                }
            }
        }

        // Cleanup
        if (_gl != null)
        {
            if (_fboWidth > 0) DeleteFboObjects();
            _gl.DeleteProgram(_program);
            _gl.DeleteVertexArray(_vao);
            _gl.DeleteBuffer(_vbo);
            _gl.Dispose();
        }
        window?.Dispose();
    }

    private void DrainChannel()
    {
        while (_channel.Reader.TryRead(out var req))
            req.Tcs.TrySetResult(null);
    }

    // ── Render ────────────────────────────────────────────────────────────────

    private byte[]? RenderInternal(List<Triangle3D> triangles, int width, int height, System.Numerics.Vector3 albedo,
        List<Triangle3D>? supportTriangles, System.Numerics.Vector3 supportAlbedo)
    {
        var gl = _gl!;

        EnsureFbo(width, height);

        // ── Camera matrices (exact port of calculateCameraDistanceForBox in ModelViewer.tsx) ──
        // Frame on all geometry (body + supports combined) so nothing is clipped.
        var framingTris = supportTriangles is { Count: > 0 }
            ? triangles.Concat(supportTriangles).ToList()
            : triangles;
        var (center, halfExtents) = BoundingBox(framingTris);
        float aspect = (float)width / height;
        float dist = CalculateCameraDistanceForBox(halfExtents, CamDir, FovY, aspect);
        var eye = center + CamDir * dist;

        var view = Matrix4x4.CreateLookAt(eye, center, System.Numerics.Vector3.UnitY);
        var proj = Matrix4x4.CreatePerspectiveFieldOfView(FovY, aspect, nearPlaneDistance: 0.1f, farPlaneDistance: dist * 4f);
        var model = Matrix4x4.Identity;

        // ── Build vertex buffer: [pos(3), normal(3)] per vertex ───────────────
        // Pre-computed flat normals live on Triangle3D.Normal; we duplicate to all 3 verts.
        int vertCount = triangles.Count * 3;
        float[] verts = BuildVertexBuffer(triangles);

        // ── GL draw ───────────────────────────────────────────────────────────
        gl.BindFramebuffer(FramebufferTarget.Framebuffer, _msaaFbo);
        gl.Viewport(0, 0, (uint)width, (uint)height);
        gl.ClearColor(0f, 0f, 0f, 0f);  // transparent
        gl.Clear(ClearBufferMask.ColorBufferBit | ClearBufferMask.DepthBufferBit);
        gl.Enable(EnableCap.DepthTest);
        gl.DepthFunc(DepthFunction.Less);

        gl.BindVertexArray(_vao);
        gl.BindBuffer(BufferTargetARB.ArrayBuffer, _vbo);
        gl.BufferData<float>(BufferTargetARB.ArrayBuffer, verts.AsSpan(), BufferUsageARB.StreamDraw);

        gl.UseProgram(_program);

        // Uniforms — upload System.Numerics matrices with transpose=false:
        // System.Numerics is row-major (row-vector convention); when passed column-by-column
        // to GLSL (which expects column-major), the raw bytes represent the transpose, which
        // is precisely the column-vector form that GLSL's left-multiply convention requires.
        SetUniformMatrix4(gl, "uModel", model);
        SetUniformMatrix4(gl, "uView", view);
        SetUniformMatrix4(gl, "uProj", proj);
        gl.Uniform3(gl.GetUniformLocation(_program, "uCameraPos"), eye.X, eye.Y, eye.Z);

        // ── Draw body ─────────────────────────────────────────────────────────
        gl.Uniform3(gl.GetUniformLocation(_program, "uAlbedo"), albedo.X, albedo.Y, albedo.Z);
        gl.DrawArrays(PrimitiveType.Triangles, 0, (uint)vertCount);

        // ── Draw supports (second pass — depth test still active) ─────────────
        if (supportTriangles is { Count: > 0 })
        {
            var suppVerts = BuildVertexBuffer(supportTriangles);
            gl.BufferData<float>(BufferTargetARB.ArrayBuffer, suppVerts.AsSpan(), BufferUsageARB.StreamDraw);
            gl.Uniform3(gl.GetUniformLocation(_program, "uAlbedo"), supportAlbedo.X, supportAlbedo.Y, supportAlbedo.Z);
            gl.DrawArrays(PrimitiveType.Triangles, 0, (uint)(supportTriangles.Count * 3));
        }

        // ── MSAA resolve ──────────────────────────────────────────────────────
        gl.BindFramebuffer(FramebufferTarget.ReadFramebuffer, _msaaFbo);
        gl.BindFramebuffer(FramebufferTarget.DrawFramebuffer, _resolveFbo);
        gl.BlitFramebuffer(
            0, 0, width, height,
            0, 0, width, height,
            ClearBufferMask.ColorBufferBit,
            BlitFramebufferFilter.Nearest);

        // ── Read pixels ───────────────────────────────────────────────────────
        gl.BindFramebuffer(FramebufferTarget.Framebuffer, _resolveFbo);
        byte[] rawPixels = new byte[width * height * 4];
        gl.ReadPixels(0, 0, (uint)width, (uint)height,
                      PixelFormat.Rgba, PixelType.UnsignedByte, rawPixels.AsSpan());

        gl.BindFramebuffer(FramebufferTarget.Framebuffer, 0);

        // ── Flip rows (OpenGL origin is bottom-left) ──────────────────────────
        int rowBytes = width * 4;
        byte[] flipped = new byte[rawPixels.Length];
        for (int y = 0; y < height; y++)
            System.Buffer.BlockCopy(rawPixels, (height - 1 - y) * rowBytes, flipped, y * rowBytes, rowBytes);

        // ── Encode PNG ────────────────────────────────────────────────────────
        using var image = Image.LoadPixelData<Rgba32>(flipped, width, height);
        using var ms = new MemoryStream();
        image.SaveAsPng(ms);
        return ms.ToArray();
    }

    private static float[] BuildVertexBuffer(List<Triangle3D> tris)
    {
        float[] buf = new float[tris.Count * 3 * 6];
        int vi = 0;
        foreach (var tri in tris)
        {
            float nx = tri.Normal.X, ny = tri.Normal.Y, nz = tri.Normal.Z;
            buf[vi++] = tri.V0.X; buf[vi++] = tri.V0.Y; buf[vi++] = tri.V0.Z;
            buf[vi++] = nx; buf[vi++] = ny; buf[vi++] = nz;
            buf[vi++] = tri.V1.X; buf[vi++] = tri.V1.Y; buf[vi++] = tri.V1.Z;
            buf[vi++] = nx; buf[vi++] = ny; buf[vi++] = nz;
            buf[vi++] = tri.V2.X; buf[vi++] = tri.V2.Y; buf[vi++] = tri.V2.Z;
            buf[vi++] = nx; buf[vi++] = ny; buf[vi++] = nz;
        }
        return buf;
    }

    private void SetUniformMatrix4(GL gl, string name, Matrix4x4 m)
    {
        int loc = gl.GetUniformLocation(_program, name);
        if (loc < 0) return;
        // Passing row-major System.Numerics matrix with transpose=false makes GLSL see it as
        // the transpose (column-major), which is exactly the column-vector form needed for GLSL.
        ReadOnlySpan<float> span = MemoryMarshal.Cast<Matrix4x4, float>(
            MemoryMarshal.CreateReadOnlySpan(ref m, 1));
        gl.UniformMatrix4(loc, 1, false, span);
    }

    // ── FBO management ────────────────────────────────────────────────────────

    private void EnsureFbo(int width, int height)
    {
        if (_fboWidth == width && _fboHeight == height) return;

        if (_fboWidth > 0) DeleteFboObjects();

        var gl = _gl!;

        // MSAA FBO (4× samples)
        _msaaFbo = gl.GenFramebuffer();
        gl.BindFramebuffer(FramebufferTarget.Framebuffer, _msaaFbo);

        _msaaColorRbo = gl.GenRenderbuffer();
        gl.BindRenderbuffer(RenderbufferTarget.Renderbuffer, _msaaColorRbo);
        gl.RenderbufferStorageMultisample(RenderbufferTarget.Renderbuffer, 4,
            InternalFormat.Rgba8, (uint)width, (uint)height);
        gl.FramebufferRenderbuffer(FramebufferTarget.Framebuffer,
            FramebufferAttachment.ColorAttachment0,
            RenderbufferTarget.Renderbuffer, _msaaColorRbo);

        _msaaDepthRbo = gl.GenRenderbuffer();
        gl.BindRenderbuffer(RenderbufferTarget.Renderbuffer, _msaaDepthRbo);
        gl.RenderbufferStorageMultisample(RenderbufferTarget.Renderbuffer, 4,
            InternalFormat.DepthComponent24, (uint)width, (uint)height);
        gl.FramebufferRenderbuffer(FramebufferTarget.Framebuffer,
            FramebufferAttachment.DepthAttachment,
            RenderbufferTarget.Renderbuffer, _msaaDepthRbo);

        // Resolve FBO (single-sample, for glReadPixels)
        _resolveFbo = gl.GenFramebuffer();
        gl.BindFramebuffer(FramebufferTarget.Framebuffer, _resolveFbo);

        _resolveColorRbo = gl.GenRenderbuffer();
        gl.BindRenderbuffer(RenderbufferTarget.Renderbuffer, _resolveColorRbo);
        gl.RenderbufferStorage(RenderbufferTarget.Renderbuffer,
            InternalFormat.Rgba8, (uint)width, (uint)height);
        gl.FramebufferRenderbuffer(FramebufferTarget.Framebuffer,
            FramebufferAttachment.ColorAttachment0,
            RenderbufferTarget.Renderbuffer, _resolveColorRbo);

        gl.BindFramebuffer(FramebufferTarget.Framebuffer, 0);

        _fboWidth = width;
        _fboHeight = height;
    }

    private void DeleteFboObjects()
    {
        var gl = _gl!;
        gl.DeleteFramebuffer(_msaaFbo);
        gl.DeleteRenderbuffer(_msaaColorRbo);
        gl.DeleteRenderbuffer(_msaaDepthRbo);
        gl.DeleteFramebuffer(_resolveFbo);
        gl.DeleteRenderbuffer(_resolveColorRbo);
    }

    // ── Shader compilation ────────────────────────────────────────────────────

    private void CompileShaders()
    {
        var gl = _gl!;

        uint vert = gl.CreateShader(ShaderType.VertexShader);
        gl.ShaderSource(vert, VertSrc);
        gl.CompileShader(vert);
        CheckShader(gl, vert, "vertex");

        uint frag = gl.CreateShader(ShaderType.FragmentShader);
        gl.ShaderSource(frag, FragSrc);
        gl.CompileShader(frag);
        CheckShader(gl, frag, "fragment");

        _program = gl.CreateProgram();
        gl.AttachShader(_program, vert);
        gl.AttachShader(_program, frag);
        gl.LinkProgram(_program);
        CheckProgram(gl, _program);

        gl.DeleteShader(vert);
        gl.DeleteShader(frag);

        _logger.LogDebug("GLSL shaders compiled & linked (program {Id})", _program);
    }

    private static void CheckShader(GL gl, uint shader, string stage)
    {
        gl.GetShader(shader, ShaderParameterName.CompileStatus, out int ok);
        if (ok == 0)
            throw new InvalidOperationException(
                $"Shader [{stage}] compile error:\n{gl.GetShaderInfoLog(shader)}");
    }

    private static void CheckProgram(GL gl, uint program)
    {
        gl.GetProgram(program, ProgramPropertyARB.LinkStatus, out int ok);
        if (ok == 0)
            throw new InvalidOperationException(
                $"Shader program link error:\n{gl.GetProgramInfoLog(program)}");
    }

    // ── VAO / VBO ─────────────────────────────────────────────────────────────

    private void SetupVao()
    {
        var gl = _gl!;
        uint stride = (uint)(6 * sizeof(float));  // pos(3) + normal(3)

        _vao = gl.GenVertexArray();
        gl.BindVertexArray(_vao);

        _vbo = gl.GenBuffer();
        gl.BindBuffer(BufferTargetARB.ArrayBuffer, _vbo);

        // location 0: position (offset 0)
        gl.EnableVertexAttribArray(0);
        gl.VertexAttribPointer(0, 3, VertexAttribPointerType.Float, false, stride, 0);

        // location 1: normal (offset 12 bytes)
        gl.EnableVertexAttribArray(1);
        gl.VertexAttribPointer(1, 3, VertexAttribPointerType.Float, false, stride, 3 * sizeof(float));

        gl.BindVertexArray(0);
    }

    // ── Camera maths (ported from ModelViewer.tsx / MeshRenderer) ────────────

    // Port of calculateCameraDistanceForBox in ModelViewer.tsx.
    // direction = unit vector FROM target TOWARD camera (same as DEFAULT_VIEW_DIRECTION).
    private static float CalculateCameraDistanceForBox(
        System.Numerics.Vector3 halfExtents,
        System.Numerics.Vector3 direction,
        float fovY, float aspect)
    {
        float halfVert = fovY / 2f;
        float halfHorz = MathF.Atan(MathF.Tan(halfVert) * aspect);
        float tanHV = MathF.Tan(halfVert);
        float tanHH = MathF.Tan(halfHorz);

        var toCamera = System.Numerics.Vector3.Normalize(direction);
        bool nearlyUp = MathF.Abs(System.Numerics.Vector3.Dot(toCamera, System.Numerics.Vector3.UnitY)) > 0.999f;
        var worldUp = nearlyUp ? System.Numerics.Vector3.UnitZ : System.Numerics.Vector3.UnitY;
        var right = System.Numerics.Vector3.Normalize(System.Numerics.Vector3.Cross(worldUp, toCamera));
        var up = System.Numerics.Vector3.Normalize(System.Numerics.Vector3.Cross(toCamera, right));

        float reqDist = 0f;
        for (int sx = -1; sx <= 1; sx += 2)
            for (int sy = -1; sy <= 1; sy += 2)
                for (int sz = -1; sz <= 1; sz += 2)
                {
                    var corner = new System.Numerics.Vector3(
                        sx * halfExtents.X, sy * halfExtents.Y, sz * halfExtents.Z);
                    float cx = MathF.Abs(System.Numerics.Vector3.Dot(corner, right));
                    float cy = MathF.Abs(System.Numerics.Vector3.Dot(corner, up));
                    float cz = System.Numerics.Vector3.Dot(corner, toCamera);
                    reqDist = MathF.Max(reqDist, MathF.Max(cz + cx / tanHH, cz + cy / tanHV));
                }
        return MathF.Max(reqDist, 0.001f) * FramingPadding;
    }

    private static (System.Numerics.Vector3 center, System.Numerics.Vector3 halfExtents)
        BoundingBox(List<Triangle3D> tris)
    {
        float minX = float.MaxValue, minY = float.MaxValue, minZ = float.MaxValue;
        float maxX = float.MinValue, maxY = float.MinValue, maxZ = float.MinValue;

        foreach (var t in tris)
        {
            Expand(t.V0); Expand(t.V1); Expand(t.V2);
        }

        var center = new System.Numerics.Vector3(
            (minX + maxX) * 0.5f, (minY + maxY) * 0.5f, (minZ + maxZ) * 0.5f);
        var half = new System.Numerics.Vector3(
            (maxX - minX) * 0.5f, (maxY - minY) * 0.5f, (maxZ - minZ) * 0.5f);
        return (center, half);

        void Expand(Vec3 v)
        {
            if (v.X < minX) minX = v.X;
            if (v.X > maxX) maxX = v.X;
            if (v.Y < minY) minY = v.Y;
            if (v.Y > maxY) maxY = v.Y;
            if (v.Z < minZ) minZ = v.Z;
            if (v.Z > maxZ) maxZ = v.Z;
        }
    }

    // ── Inner types ───────────────────────────────────────────────────────────

    private sealed record RenderRequest(
        TaskCompletionSource<byte[]?> Tcs,
        List<Triangle3D> Triangles,
        int Width,
        int Height,
        System.Numerics.Vector3 Albedo,
        List<Triangle3D>? SupportTriangles,
        System.Numerics.Vector3 SupportAlbedo);
}
