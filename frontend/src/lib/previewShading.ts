import * as THREE from 'three';

// Port of the preview renderer's shading (backend/src/preview/shader.wgsl and
// camera::shade) so the 3D viewer matches the rendered previews. Keep the constants and
// lights in step with backend/src/preview/camera.rs.
//
// Like the preview, the lighting runs directly on the sRGB colour with no tone mapping
// or colour-space conversion, uses the face normal from the triangle winding, and draws
// back faces.

const VERTEX_SHADER = /* glsl */ `
varying vec3 vWorld;

void main() {
  vec4 world = modelMatrix * vec4(position, 1.0);
  vWorld = world.xyz;
  gl_Position = projectionMatrix * viewMatrix * world;
}
`;

const FRAGMENT_SHADER = /* glsl */ `
uniform vec3 albedo;
uniform float opacity;
varying vec3 vWorld;

const float AMBIENT = 0.40;
const float DIFFUSE = 0.70;
const float SPECULAR = 0.15;
const float SHININESS = 20.0;

void main() {
  vec3 lightDirs[3];
  lightDirs[0] = normalize(vec3(5.0, 8.0, 5.0));
  lightDirs[1] = normalize(vec3(-4.0, 2.0, -2.0));
  lightDirs[2] = normalize(vec3(0.0, -3.0, -5.0));
  float intensities[3];
  intensities[0] = 1.20;
  intensities[1] = 0.40;
  intensities[2] = 0.25;

  // Screen-space derivatives give the face normal facing the camera; flipping it on back
  // faces gives the winding normal the preview uses.
  vec3 n = normalize(cross(dFdx(vWorld), dFdy(vWorld)));
  if (!gl_FrontFacing) n = -n;
  vec3 v = normalize(cameraPosition - vWorld);

  vec3 colour = AMBIENT * albedo;
  for (int i = 0; i < 3; i++) {
    float diff = max(dot(n, lightDirs[i]), 0.0);
    colour += DIFFUSE * diff * intensities[i] * albedo;
    vec3 r = reflect(-lightDirs[i], n);
    float spec = pow(max(dot(v, r), 0.0), SHININESS);
    colour += vec3(SPECULAR * spec * intensities[i]);
  }
  gl_FragColor = vec4(clamp(colour, 0.0, 1.0), opacity);
}
`;

/** Raw sRGB channels of a `#rrggbb` colour, without three's linear conversion. */
function srgbChannels(hex: string): THREE.Vector3 {
  const value = parseInt(hex.replace('#', ''), 16);
  return new THREE.Vector3(
    ((value >> 16) & 0xff) / 255,
    ((value >> 8) & 0xff) / 255,
    (value & 0xff) / 255,
  );
}

export function createPreviewMaterial(hex: string, opacity = 1): THREE.ShaderMaterial {
  return new THREE.ShaderMaterial({
    vertexShader: VERTEX_SHADER,
    fragmentShader: FRAGMENT_SHADER,
    uniforms: {
      albedo: { value: srgbChannels(hex) },
      opacity: { value: opacity },
    },
    side: THREE.DoubleSide,
    transparent: opacity < 1,
  });
}
