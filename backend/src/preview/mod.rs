//! PNG preview generation for 3D geometry.
//!
//! Rendering path (in priority order):
//!   1. [`gpu::GpuRenderer`]: wgpu hardware renderer with 4x MSAA.
//!   2. [`cpu::render`]: software rasteriser fallback, used when no graphics adapter is
//!      available, the GPU render fails, or GPU rendering is disabled in settings.

pub mod camera;
pub mod cpu;
pub mod gpu;

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use tracing::{debug, info, warn};

#[cfg(test)]
use crate::geometry::hull::DEFAULT_RAFT_HEIGHT_MM;
use crate::geometry::{LoadedGeometry, Triangle, Vec3, support_separation};

const LOG_TARGET: &str = "findamodel.Preview";

/// Increment when the preview rendering implementation changes so existing cached
/// previews are invalidated on the next scan.
pub const CURRENT_PREVIEW_GENERATION_VERSION: i32 = 14;

const RENDER_WIDTH: u32 = 512;
const RENDER_HEIGHT: u32 = 512;
const SUPPORTS_REMOVED_SUFFIX: &str = "_supportsremoved";

// Colours matching ModelViewer.tsx: body = #a8404f (oxblood), supports = #c9a45c (brass).
const BODY_COLOR: Vec3 = Vec3::new(0xa8 as f32 / 255.0, 0x40 as f32 / 255.0, 0x4f as f32 / 255.0);
const SUPPORT_COLOR: Vec3 = Vec3::new(0xc9 as f32 / 255.0, 0xa4 as f32 / 255.0, 0x5c as f32 / 255.0);

/// File stem for a model's previews. The support split depends on the raft height and the
/// supported flag, so both are part of the name: a change renders new files instead of
/// reusing ones split the old way.
pub fn preview_stem(model_file_hash: &str, raft_height_mm: f32, supported: Option<bool>) -> String {
    let split = match supported {
        Some(true) => "s",
        Some(false) => "n",
        None => "a",
    };
    format!("{model_file_hash}_r{raft_height_mm:.2}{split}")
}

/// The stem of a stored with-supports preview path.
pub fn stem_of(relative_path: &str) -> &str {
    relative_path.strip_suffix(".png").unwrap_or(relative_path)
}

pub fn relative_path(stem: &str, include_supports: bool) -> String {
    if include_supports { format!("{stem}.png") } else { format!("{stem}{SUPPORTS_REMOVED_SUFFIX}.png") }
}

pub fn encode_png(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().expect("png header");
        writer.write_image_data(rgba).expect("png data");
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewResult {
    pub relative_path: Option<String>,
    pub generated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewVariants {
    pub with_supports: PreviewResult,
    pub without_supports: PreviewResult,
}

/// Runtime state of the GPU renderer, initialised once in the background.
#[derive(Default)]
pub struct GpuState {
    renderer: OnceLock<Result<Arc<gpu::GpuRenderer>, String>>,
}

impl GpuState {
    /// Initialises the renderer on a background thread; safe to call more than once.
    pub fn initialize(self: &Arc<Self>) {
        let state = self.clone();
        std::thread::spawn(move || {
            state.renderer.get_or_init(|| match gpu::GpuRenderer::new() {
                Ok(r) => Ok(Arc::new(r)),
                Err(err) => {
                    warn!(target: LOG_TARGET, "GPU preview renderer unavailable, using CPU rasteriser: {err:#}");
                    Err(format!("{err:#}"))
                }
            });
        });
    }

    pub fn renderer(&self) -> Option<Arc<gpu::GpuRenderer>> {
        self.renderer.get().and_then(|r| r.as_ref().ok()).cloned()
    }

    pub fn is_available(&self) -> bool {
        self.renderer().is_some()
    }

    /// The GPU in use, or why previews fall back to the CPU rasteriser.
    pub fn renderer_name(&self) -> String {
        match self.renderer.get() {
            Some(Ok(r)) => r.renderer_name().to_string(),
            Some(Err(reason)) => format!("CPU rasteriser ({reason})"),
            None => "CPU rasteriser".to_string(),
        }
    }
}

#[derive(Clone)]
pub struct PreviewService {
    cache_dir: Option<PathBuf>,
    use_gpu: bool,
    gpu: Arc<GpuState>,
}

impl PreviewService {
    pub fn new(cache_dir: Option<PathBuf>, use_gpu: bool, gpu: Arc<GpuState>) -> Self {
        Self { cache_dir, use_gpu, gpu }
    }

    pub fn cache_dir(&self) -> Option<&Path> {
        self.cache_dir.as_deref()
    }

    pub fn gpu_enabled(&self) -> bool {
        self.use_gpu
    }

    pub fn gpu(&self) -> &Arc<GpuState> {
        &self.gpu
    }

    /// Renders whichever requested variants are missing from the cache and saves them,
    /// tinting the triangles flagged in `is_support`. Cached files matching the stem are
    /// reused. Blocking; run on a blocking thread.
    pub fn generate_variants(
        &self,
        geometry: &LoadedGeometry,
        stem: &str,
        is_support: Option<&[bool]>,
        generate_with_supports: bool,
        generate_without_supports: bool,
    ) -> PreviewVariants {
        let none = PreviewVariants {
            with_supports: PreviewResult { relative_path: None, generated: false },
            without_supports: PreviewResult { relative_path: None, generated: false },
        };
        if geometry.triangles.is_empty() {
            warn!(target: LOG_TARGET, "Preview generation called with empty geometry");
            return none;
        }
        let Some(cache_dir) = &self.cache_dir else {
            return none;
        };

        let with_path = relative_path(stem, true);
        let without_path = relative_path(stem, false);
        let with_full = cache_dir.join(&with_path);
        let without_full = cache_dir.join(&without_path);
        let with_exists = with_full.exists();
        let without_exists = without_full.exists();

        let mut result = PreviewVariants {
            with_supports: PreviewResult {
                relative_path: generate_with_supports.then(|| with_path.clone()),
                generated: false,
            },
            without_supports: PreviewResult {
                relative_path: generate_without_supports.then(|| without_path.clone()),
                generated: false,
            },
        };

        let needs_with = generate_with_supports && !with_exists;
        let needs_without = generate_without_supports && !without_exists;
        if !needs_with && !needs_without {
            debug!(target: LOG_TARGET, "Reusing cached previews for {stem}");
            return result;
        }

        // Separate body from supports so they can be rendered in distinct colours.
        let (body, supports) = match is_support {
            Some(mask) => {
                let (body, supports) = support_separation::split(&geometry.triangles, mask);
                (body, Some(supports))
            }
            None => (geometry.triangles.clone(), None),
        };
        info!(
            target: LOG_TARGET,
            "Rendering preview ({} body triangles, {} support triangles)",
            body.len(),
            supports.as_ref().map_or(0, Vec::len)
        );

        let save = |path: &Path, png: &[u8]| -> bool {
            match std::fs::write(path, png) {
                Ok(()) => {
                    info!(target: LOG_TARGET, "Saved preview to {}", path.display());
                    true
                }
                Err(err) => {
                    warn!(target: LOG_TARGET, "Failed to save preview to {}: {err}", path.display());
                    false
                }
            }
        };

        if needs_with && needs_without {
            let (model_only, with_supports) = self.render_pair(&body, supports.as_deref());
            if save(&with_full, &with_supports) {
                result.with_supports = PreviewResult { relative_path: Some(with_path), generated: true };
            }
            if save(&without_full, &model_only) {
                result.without_supports = PreviewResult { relative_path: Some(without_path), generated: true };
            }
            return result;
        }

        if needs_with {
            let png = self.render_single(&body, supports.as_deref());
            if save(&with_full, &png) {
                result.with_supports = PreviewResult { relative_path: Some(with_path), generated: true };
            }
        }
        if needs_without {
            let png = self.render_single(&body, None);
            if save(&without_full, &png) {
                result.without_supports = PreviewResult { relative_path: Some(without_path), generated: true };
            }
        }
        result
    }

    fn gpu_renderer(&self) -> Option<Arc<gpu::GpuRenderer>> {
        if self.use_gpu { self.gpu.renderer() } else { None }
    }

    fn render_pair(&self, body: &[Triangle], supports: Option<&[Triangle]>) -> (Vec<u8>, Vec<u8>) {
        if let Some(renderer) = self.gpu_renderer() {
            match renderer.render_variant_pair(body, supports, RENDER_WIDTH, RENDER_HEIGHT, BODY_COLOR, SUPPORT_COLOR) {
                Ok(pair) => return pair,
                Err(err) => warn!(target: LOG_TARGET, "GPU dual preview render failed; falling back to CPU: {err:#}"),
            }
        }
        let (w, h) = (RENDER_WIDTH as usize, RENDER_HEIGHT as usize);
        (
            cpu::render(body, w, h, BODY_COLOR, None, SUPPORT_COLOR),
            cpu::render(body, w, h, BODY_COLOR, supports, SUPPORT_COLOR),
        )
    }

    fn render_single(&self, body: &[Triangle], supports: Option<&[Triangle]>) -> Vec<u8> {
        if let Some(renderer) = self.gpu_renderer() {
            match renderer.render(body, supports, RENDER_WIDTH, RENDER_HEIGHT, BODY_COLOR, SUPPORT_COLOR) {
                Ok(png) => return png,
                Err(err) => warn!(target: LOG_TARGET, "GPU preview render failed; falling back to CPU: {err:#}"),
            }
        }
        cpu::render(body, RENDER_WIDTH as usize, RENDER_HEIGHT as usize, BODY_COLOR, supports, SUPPORT_COLOR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube_with_support() -> LoadedGeometry {
        let mut triangles = Vec::new();
        let quad = |a: Vec3, b: Vec3, c: Vec3, d: Vec3, out: &mut Vec<Triangle>| {
            out.push(Triangle::new(a, b, c, Vec3::UP));
            out.push(Triangle::new(a, c, d, Vec3::UP));
        };
        let p = |x: f32, y: f32, z: f32| Vec3::new(x, y + 2.0, z);
        // Unit-ish cube body (12 triangles) raised above a small detached support pillar.
        let (s, h) = (10.0, 10.0);
        quad(p(-s, 0.0, -s), p(s, 0.0, -s), p(s, 0.0, s), p(-s, 0.0, s), &mut triangles);
        quad(p(-s, h, -s), p(-s, h, s), p(s, h, s), p(s, h, -s), &mut triangles);
        quad(p(-s, 0.0, -s), p(-s, h, -s), p(s, h, -s), p(s, 0.0, -s), &mut triangles);
        quad(p(-s, 0.0, s), p(s, 0.0, s), p(s, h, s), p(-s, h, s), &mut triangles);
        quad(p(-s, 0.0, -s), p(-s, 0.0, s), p(-s, h, s), p(-s, h, -s), &mut triangles);
        quad(p(s, 0.0, -s), p(s, h, -s), p(s, h, s), p(s, 0.0, s), &mut triangles);
        for _ in 0..4 {
            let body = triangles.clone();
            triangles.extend(body);
        }
        triangles.push(Triangle::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 2.0, 0.0),
            Vec3::UP,
        ));
        triangles.push(Triangle::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 2.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::UP,
        ));
        triangles.push(Triangle::new(
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 2.0, 0.0),
            Vec3::UP,
        ));
        triangles.push(Triangle::new(
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::UP,
        ));
        LoadedGeometry {
            triangles,
            sphere_centre: Vec3::new(0.0, 6.0, 0.0),
            sphere_radius: 16.0,
            dimension_x_mm: 20.0,
            dimension_y_mm: 12.0,
            dimension_z_mm: 20.0,
        }
    }

    fn support_mask(geometry: &LoadedGeometry) -> Vec<bool> {
        support_separation::classify(&geometry.triangles, DEFAULT_RAFT_HEIGHT_MM)
            .and_then(|c| c.mask_for(Some(true)))
            .expect("support mask")
    }

    #[test]
    fn stem_encodes_raft_height_and_supported_flag() {
        assert_eq!(preview_stem("abc", 2.0, None), "abc_r2.00a");
        assert_eq!(preview_stem("abc", 1.5, Some(true)), "abc_r1.50s");
        assert_eq!(preview_stem("abc", 1.5, Some(false)), "abc_r1.50n");
        assert_eq!(relative_path("abc_r2.00a", false), "abc_r2.00a_supportsremoved.png");
        assert_eq!(stem_of("abc_r2.00a.png"), "abc_r2.00a");
    }

    fn decode(png_bytes: &[u8]) -> (u32, u32, Vec<u8>) {
        let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).unwrap();
        (info.width, info.height, buf[..info.buffer_size()].to_vec())
    }

    fn opaque_fraction(rgba: &[u8]) -> f64 {
        rgba.chunks(4).filter(|p| p[3] == 255).count() as f64 / (rgba.len() / 4) as f64
    }

    #[test]
    fn cpu_render_produces_framed_image_with_transparent_background() {
        let geometry = cube_with_support();
        let png = cpu::render(&geometry.triangles, 128, 128, BODY_COLOR, None, SUPPORT_COLOR);
        let (w, h, rgba) = decode(&png);
        assert_eq!((w, h), (128, 128));
        assert_eq!(rgba[3], 0, "corner pixel should be transparent");
        let coverage = opaque_fraction(&rgba);
        assert!(coverage > 0.2 && coverage < 0.95, "coverage {coverage}");
    }

    #[test]
    fn generate_variants_writes_both_files_and_reuses_cache() {
        let dir = tempfile::tempdir().unwrap();
        let service = PreviewService::new(Some(dir.path().to_path_buf()), false, Arc::new(GpuState::default()));
        let geometry = cube_with_support();

        let mask = support_mask(&geometry);
        let first = service.generate_variants(&geometry, "abc", Some(&mask), true, true);
        assert!(first.with_supports.generated && first.without_supports.generated);
        assert!(dir.path().join("abc.png").exists());
        assert!(dir.path().join("abc_supportsremoved.png").exists());

        let second = service.generate_variants(&geometry, "abc", Some(&mask), true, true);
        assert!(!second.with_supports.generated && !second.without_supports.generated);
        assert_eq!(second.with_supports.relative_path.as_deref(), Some("abc.png"));
    }

    #[test]
    fn gpu_render_matches_cpu_coverage_when_adapter_available() {
        let Ok(renderer) = gpu::GpuRenderer::new_allowing_software() else {
            eprintln!("skipping: no graphics adapter");
            return;
        };
        let geometry = cube_with_support();
        let (body, supports) = support_separation::separate(&geometry.triangles, DEFAULT_RAFT_HEIGHT_MM, Some(true));
        let (model_only, with_supports) =
            renderer.render_variant_pair(&body, supports.as_deref(), 128, 128, BODY_COLOR, SUPPORT_COLOR).unwrap();

        let (_, _, gpu_rgba) = decode(&with_supports);
        let (_, _, cpu_rgba) = decode(&cpu::render(&body, 128, 128, BODY_COLOR, supports.as_deref(), SUPPORT_COLOR));
        let (gpu_cov, cpu_cov) = (opaque_fraction(&gpu_rgba), opaque_fraction(&cpu_rgba));
        assert!((gpu_cov - cpu_cov).abs() < 0.08, "gpu {gpu_cov} vs cpu {cpu_cov}");
        assert!(!model_only.is_empty());
    }

    #[test]
    fn gpu_chunked_draw_matches_single_draw() {
        let Ok(renderer) = gpu::GpuRenderer::new_allowing_software() else {
            eprintln!("skipping: no graphics adapter");
            return;
        };
        let geometry = cube_with_support();
        let single = renderer.render(&geometry.triangles, None, 64, 64, BODY_COLOR, SUPPORT_COLOR).unwrap();
        let chunked = renderer.render_chunked_for_test(&geometry.triangles, 7, 64, 64, BODY_COLOR).unwrap();
        assert_eq!(decode(&single).2, decode(&chunked).2);
    }

    /// Renders a dense synthetic mesh with both renderers. Run with
    /// `cargo test -p findamodel --release -- --ignored preview_benchmark --nocapture`.
    #[test]
    #[ignore]
    fn preview_benchmark() {
        let grid = 700usize;
        let mut triangles = Vec::with_capacity(grid * grid * 2);
        for i in 0..grid {
            for j in 0..grid {
                let h = |x: usize, z: usize| ((x as f32 * 0.05).sin() + (z as f32 * 0.07).cos()) * 5.0 + 10.0;
                let v = |x: usize, z: usize| Vec3::new(x as f32 * 0.1, h(x, z), z as f32 * 0.1);
                triangles.push(Triangle::new(v(i, j), v(i + 1, j), v(i + 1, j + 1), Vec3::UP));
                triangles.push(Triangle::new(v(i, j), v(i + 1, j + 1), v(i, j + 1), Vec3::UP));
            }
        }
        eprintln!("benchmark triangles: {}", triangles.len());

        let start = std::time::Instant::now();
        let _ = cpu::render(&triangles, 512, 512, BODY_COLOR, None, SUPPORT_COLOR);
        eprintln!("cpu render: {:?}", start.elapsed());

        match gpu::GpuRenderer::new_allowing_software() {
            Ok(renderer) => {
                let _ = renderer.render(&triangles, None, 512, 512, BODY_COLOR, SUPPORT_COLOR).unwrap();
                let start = std::time::Instant::now();
                let _ = renderer.render(&triangles, None, 512, 512, BODY_COLOR, SUPPORT_COLOR).unwrap();
                eprintln!("gpu render ({}): {:?}", renderer.renderer_name(), start.elapsed());
            }
            Err(err) => eprintln!("gpu unavailable: {err:#}"),
        }
    }
}
