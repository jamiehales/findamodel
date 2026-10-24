// Preview shader. Lighting matches the frontend viewer (ModelViewer.tsx):
//   ambient 0.4, directional lights [5,8,5] x1.2, [-4,2,-2] x0.4, [0,-3,-5] x0.25.

struct Uniforms {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    albedo: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) world: vec3<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>) -> VsOut {
    var out: VsOut;
    out.position = u.view_proj * vec4<f32>(position, 1.0);
    out.normal = normal;
    out.world = position;
    return out;
}

const AMBIENT: f32 = 0.40;
const DIFFUSE: f32 = 0.70;
const SPECULAR: f32 = 0.15;
const SHININESS: f32 = 20.0;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let light_dirs = array<vec3<f32>, 3>(
        normalize(vec3<f32>(5.0, 8.0, 5.0)),
        normalize(vec3<f32>(-4.0, 2.0, -2.0)),
        normalize(vec3<f32>(0.0, -3.0, -5.0)),
    );
    let intensities = array<f32, 3>(1.20, 0.40, 0.25);

    let n = normalize(in.normal);
    let v = normalize(u.camera_pos.xyz - in.world);
    var colour = AMBIENT * u.albedo.rgb;
    for (var i = 0u; i < 3u; i = i + 1u) {
        let diff = max(dot(n, light_dirs[i]), 0.0);
        colour = colour + DIFFUSE * diff * intensities[i] * u.albedo.rgb;
        let r = reflect(-light_dirs[i], n);
        let spec = pow(max(dot(v, r), 0.0), SHININESS);
        colour = colour + vec3<f32>(SPECULAR * spec * intensities[i]);
    }
    return vec4<f32>(clamp(colour, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
