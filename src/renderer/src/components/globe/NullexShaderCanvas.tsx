import React, { useEffect, useRef } from 'react'

export type NullexShaderPreset = 'galaxy' | 'color-bends' | 'simple-gradient'

interface NullexShaderCanvasProps {
  preset: NullexShaderPreset
  className?: string
}

const VERTEX_SHADER_SRC = `#version 300 es
in vec2 aPos;
void main() {
  gl_Position = vec4(aPos, 0.0, 1.0);
}
`

const GALAXY_FRAGMENT_SRC = `#version 300 es
precision highp float;

uniform vec2 uSize;
uniform float uTime;
out vec4 fragColor;

#define NUM_LAYER 4.0
#define STAR_COLOR_CUTOFF 0.2
#define MAT45 mat2(0.7071, -0.7071, 0.7071, 0.7071)
#define PERIOD 3.0

const float DENSITY = 1.0;
const float HUE_SHIFT = 140.0;
const float SPEED = 0.35;
const float GLOW_INTENSITY = 0.3;
const float SATURATION = 0.0;
const float TWINKLE_INTENSITY = 0.3;
const float ROTATION_SPEED = 0.03;
const float STAR_SPEED = 0.15;

float Hash21(vec2 p) {
  p = fract(p * vec2(123.34, 456.21));
  p += dot(p, p + 45.32);
  return fract(p.x * p.y);
}

float tri(float x) {
  return abs(fract(x) * 2.0 - 1.0);
}

float tris(float x) {
  float t = fract(x);
  return 1.0 - smoothstep(0.0, 1.0, abs(2.0 * t - 1.0));
}

float trisn(float x) {
  float t = fract(x);
  return 2.0 * (1.0 - smoothstep(0.0, 1.0, abs(2.0 * t - 1.0))) - 1.0;
}

vec3 hsv2rgb(vec3 c) {
  vec4 K = vec4(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
  vec3 p = abs(fract(c.xxx + K.xyz) * 6.0 - K.www);
  return c.z * mix(K.xxx, clamp(p - K.xxx, 0.0, 1.0), c.y);
}

float Star(vec2 uv, float flare) {
  float d = length(uv) + 1e-4;
  float m = (0.05 * GLOW_INTENSITY) / d;
  float rays = smoothstep(0.0, 1.0, 1.0 - abs(uv.x * uv.y * 1000.0));
  m += rays * flare * GLOW_INTENSITY;
  uv *= MAT45;
  rays = smoothstep(0.0, 1.0, 1.0 - abs(uv.x * uv.y * 1000.0));
  m += rays * 0.3 * flare * GLOW_INTENSITY;
  m *= smoothstep(1.0, 0.2, d);
  return m;
}

vec3 StarLayer(vec2 uv, float starSpeedValue) {
  vec3 col = vec3(0.0);
  vec2 gv = fract(uv) - 0.5;
  vec2 id = floor(uv);

  for (int y = -1; y <= 1; y++) {
    for (int x = -1; x <= 1; x++) {
      vec2 offset = vec2(float(x), float(y));
      vec2 si = id + offset;
      float seed = Hash21(si);
      float size = fract(seed * 345.32);
      float glossLocal = tri(starSpeedValue / (PERIOD * seed + 1.0));
      float flareSize = smoothstep(0.9, 1.0, size) * glossLocal;

      float red = smoothstep(STAR_COLOR_CUTOFF, 1.0, Hash21(si + 1.0)) + STAR_COLOR_CUTOFF;
      float blu = smoothstep(STAR_COLOR_CUTOFF, 1.0, Hash21(si + 3.0)) + STAR_COLOR_CUTOFF;
      float grn = min(red, blu) * seed;
      vec3 base = vec3(red, grn, blu);

      float hue = atan(base.g - base.r, base.b - base.r) / (2.0 * 3.14159) + 0.5;
      hue = fract(hue + HUE_SHIFT / 360.0);
      float sat = length(base - vec3(dot(base, vec3(0.299, 0.587, 0.114)))) * SATURATION;
      float val = max(max(base.r, base.g), base.b);
      base = hsv2rgb(vec3(hue, sat, val));

      vec2 pad = vec2(
        tris(seed * 34.0 + uTime * SPEED / 10.0),
        tris(seed * 38.0 + uTime * SPEED / 30.0)
      ) - 0.5;

      float star = Star(gv - offset - pad, flareSize);
      float twinkle = trisn(uTime * SPEED + seed * 6.2831) * 0.5 + 1.0;
      twinkle = mix(1.0, twinkle, TWINKLE_INTENSITY);
      star *= twinkle;

      col += star * size * base;
    }
  }
  return col;
}

void main() {
  vec2 fragCoord = vec2(gl_FragCoord.x, uSize.y - gl_FragCoord.y);
  vec2 uv = (fragCoord - 0.5 * uSize) / max(uSize.y, 1.0);

  float autoRotAngle = uTime * ROTATION_SPEED;
  mat2 autoRot = mat2(
    cos(autoRotAngle), -sin(autoRotAngle),
    sin(autoRotAngle), cos(autoRotAngle)
  );
  uv = autoRot * uv;

  float starSpeedValue = uTime * STAR_SPEED;
  vec3 col = vec3(0.0);

  for (float i = 0.0; i < 1.0; i += 1.0 / NUM_LAYER) {
    float depth = fract(i + starSpeedValue * SPEED);
    float scale = mix(20.0 * DENSITY, 0.5 * DENSITY, depth);
    float fade = depth * smoothstep(1.0, 0.9, depth);
    col += StarLayer(uv * scale + i * 453.32, starSpeedValue) * fade;
  }

  fragColor = vec4(col + vec3(0.0392), 1.0);
}
`

const COLOR_BENDS_FRAGMENT_SRC = `#version 300 es
precision highp float;

uniform vec2 uSize;
uniform float uTime;
out vec4 fragColor;

const int ITERATIONS = 5;
const float SPEED = 0.12;
const float SCALE = 1.0;
const float FREQUENCY = 1.2;
const float WARP_STRENGTH = 1.0;
const float BAND_WIDTH = 4.0;
const float INTENSITY = 1.3;

void main() {
  float t = uTime * SPEED;
  vec2 fragCoord = vec2(gl_FragCoord.x, uSize.y - gl_FragCoord.y);
  vec2 uv = fragCoord / max(uSize, vec2(1.0));
  vec2 p = uv * 2.0 - 1.0;
  vec2 q = vec2(p.x * (uSize.x / max(uSize.y, 1.0)), p.y) / SCALE;
  q /= 0.5 + 0.2 * dot(q, q);
  q += 0.2 * cos(t) - 7.56;

  for (int j = 0; j < ITERATIONS; j++) {
    vec2 rr = sin(1.5 * (q.yx * FREQUENCY) + 2.0 * cos(q * FREQUENCY));
    q += (rr - q) * 0.15;
  }

  vec3 colors[2];
  colors[0] = vec3(0.55, 0.25, 0.95);
  colors[1] = vec3(0.75, 0.20, 0.85);

  vec3 col = vec3(0.0);
  float cover = 0.0;
  vec2 s = q;

  for (int i = 0; i < 2; i++) {
    s -= 0.01;
    vec2 r = sin(1.5 * (s.yx * FREQUENCY) + 2.0 * cos(s * FREQUENCY));
    float m0 = length(r + sin(5.0 * r.y * FREQUENCY - 3.0 * t + float(i)) / 4.0);
    vec2 disp = (r - s) * WARP_STRENGTH;
    vec2 warped = s + disp;
    float m1 = length(warped + sin(5.0 * warped.y * FREQUENCY - 3.0 * t + float(i)) / 4.0);
    float m = mix(m0, m1, 0.6);
    float w = 1.0 - exp(-BAND_WIDTH / exp(BAND_WIDTH * m));
    col += colors[i] * w;
    cover = max(cover, w);
  }

  col = clamp(col, 0.0, 1.0) * INTENSITY;
  vec3 bg = vec3(0.0392);
  fragColor = vec4(mix(bg, col, clamp(cover * INTENSITY, 0.0, 1.0)), 1.0);
}
`

const SIMPLE_GRADIENT_FRAGMENT_SRC = `#version 300 es
precision highp float;

uniform vec2 uSize;
uniform float uTime;
out vec4 fragColor;

const int ITERATIONS = 5;
const float SPEED = 0.12;
const float SCALE = 1.0;
const float FREQUENCY = 1.2;
const float WARP_STRENGTH = 1.0;
const float BAND_WIDTH = 1.6;
const float INTENSITY = 0.5;

void main() {
  float t = uTime * SPEED;
  vec2 fragCoord = vec2(gl_FragCoord.x, uSize.y - gl_FragCoord.y);
  vec2 uv = fragCoord / max(uSize, vec2(1.0));
  vec2 p = uv * 2.0 - 1.0;
  vec2 q = vec2(p.x * (uSize.x / max(uSize.y, 1.0)), p.y) / SCALE;
  q /= 0.5 + 0.2 * dot(q, q);
  q += 0.2 * cos(t) - 7.56;

  for (int j = 0; j < ITERATIONS; j++) {
    vec2 rr = sin(1.5 * (q.yx * FREQUENCY) + 2.0 * cos(q * FREQUENCY));
    q += (rr - q) * 0.15;
  }

  vec3 colors[3];
  colors[0] = vec3(0.42, 0.35, 0.95);
  colors[1] = vec3(0.25, 0.55, 0.95);
  colors[2] = vec3(0.30, 0.85, 0.85);

  vec3 col = vec3(0.0);
  float cover = 0.0;
  vec2 s = q;

  for (int i = 0; i < 3; i++) {
    s -= 0.01;
    vec2 r = sin(1.5 * (s.yx * FREQUENCY) + 2.0 * cos(s * FREQUENCY));
    float m0 = length(r + sin(5.0 * r.y * FREQUENCY - 3.0 * t + float(i)) / 4.0);
    vec2 disp = (r - s) * WARP_STRENGTH;
    vec2 warped = s + disp;
    float m1 = length(warped + sin(5.0 * warped.y * FREQUENCY - 3.0 * t + float(i)) / 4.0);
    float m = mix(m0, m1, 0.6);
    float w = 1.0 - exp(-BAND_WIDTH / exp(BAND_WIDTH * m));
    col += colors[i] * w;
    cover = max(cover, w);
  }

  col = clamp(col, 0.0, 1.0) * INTENSITY;
  vec3 bg = vec3(0.0392);
  fragColor = vec4(mix(bg, col, clamp(cover * INTENSITY, 0.0, 1.0)), 1.0);
}
`

function compileShader(gl: WebGL2RenderingContext, type: number, source: string): WebGLShader | null {
  const shader = gl.createShader(type)
  if (!shader) return null
  gl.shaderSource(shader, source)
  gl.compileShader(shader)
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    gl.deleteShader(shader)
    return null
  }
  return shader
}

export const NullexShaderCanvas: React.FC<NullexShaderCanvasProps> = ({ preset, className = '' }) => {
  const canvasRef = useRef<HTMLCanvasElement>(null)

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const gl = canvas.getContext('webgl2', { alpha: false, antialias: false })
    if (!gl) return

    const fragSrc =
      preset === 'color-bends'
        ? COLOR_BENDS_FRAGMENT_SRC
        : preset === 'simple-gradient'
          ? SIMPLE_GRADIENT_FRAGMENT_SRC
          : GALAXY_FRAGMENT_SRC

    const vs = compileShader(gl, gl.VERTEX_SHADER, VERTEX_SHADER_SRC)
    const fs = compileShader(gl, gl.FRAGMENT_SHADER, fragSrc)
    if (!vs || !fs) {
      if (vs) gl.deleteShader(vs)
      if (fs) gl.deleteShader(fs)
      return
    }

    const program = gl.createProgram()
    if (!program) {
      gl.deleteShader(vs)
      gl.deleteShader(fs)
      return
    }
    gl.attachShader(program, vs)
    gl.attachShader(program, fs)
    gl.linkProgram(program)

    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      gl.deleteProgram(program)
      gl.deleteShader(vs)
      gl.deleteShader(fs)
      return
    }

    const quadBuffer = gl.createBuffer()
    gl.bindBuffer(gl.ARRAY_BUFFER, quadBuffer)
    gl.bufferData(
      gl.ARRAY_BUFFER,
      new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]),
      gl.STATIC_DRAW
    )

    const aPosLoc = gl.getAttribLocation(program, 'aPos')
    const uSizeLoc = gl.getUniformLocation(program, 'uSize')
    const uTimeLoc = gl.getUniformLocation(program, 'uTime')

    const updateSize = (): void => {
      const rect = canvas.getBoundingClientRect()
      const dpr = Math.min(window.devicePixelRatio || 1, 2)
      const w = Math.max(1, Math.floor(rect.width * dpr))
      const h = Math.max(1, Math.floor(rect.height * dpr))
      if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w
        canvas.height = h
      }
    }
    updateSize()
    const observer = new ResizeObserver(updateSize)
    observer.observe(canvas)

    let rafId = 0
    const startTime = performance.now()

    const render = (now: number): void => {
      const width = canvas.width
      const height = canvas.height
      if (width > 0 && height > 0) {
        gl.viewport(0, 0, width, height)
        gl.useProgram(program)
        gl.bindBuffer(gl.ARRAY_BUFFER, quadBuffer)
        gl.enableVertexAttribArray(aPosLoc)
        gl.vertexAttribPointer(aPosLoc, 2, gl.FLOAT, false, 0, 0)
        gl.uniform2f(uSizeLoc, width, height)
        gl.uniform1f(uTimeLoc, (now - startTime) * 0.001)
        gl.drawArrays(gl.TRIANGLES, 0, 6)
      }
      rafId = requestAnimationFrame(render)
    }

    rafId = requestAnimationFrame(render)

    return () => {
      cancelAnimationFrame(rafId)
      observer.disconnect()
      gl.deleteBuffer(quadBuffer)
      gl.deleteProgram(program)
      gl.deleteShader(vs)
      gl.deleteShader(fs)
    }
  }, [preset])

  return (
    <canvas
      ref={canvasRef}
      className={`pointer-events-none absolute inset-0 w-full h-full block ${className}`}
    />
  )
}

export default NullexShaderCanvas
