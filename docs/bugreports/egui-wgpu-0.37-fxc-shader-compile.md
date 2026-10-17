# Черновик issue для emilk/egui — egui-wgpu 0.37: пайплайн компилируется на DX12 в 4 раза дольше

Статус: опубликован 2026-10-17 с разрешения владельца — https://github.com/emilk/egui/issues/8736
Наш обход — `vendor/egui-wgpu/PATCH.md`.

---

**Title:** egui-wgpu 0.37: `wrap_texel_coord` makes the egui pipeline ~4× slower to compile on DX12 (FXC), +40 ms startup

**Describe the bug**

Since 0.37 the egui render pipeline takes ~55–65 ms to create on DX12 instead of ~10–15 ms in 0.36. This happens on the main thread before the first frame, so native app startup gets ~40 ms slower on Windows.

The cause is the new `wrap_texel_coord` in `egui.wgsl`, which is in the `predictable_texture_filtering` branch. It uses integer `%` and is inlined five times (one nearest tap and four bilinear taps), each time with an `if`/`else if` on the wrap mode. naga's HLSL backend turns each integer `%` into a checked helper function, and FXC (the default DX12 compiler; DXC isn't shipped with Windows) is slow to optimize the result. `predictable_texture_filtering` is a runtime uniform, so the branch is compiled even when the option is off, which is the default.

**To Reproduce**

1. On Windows with the DX12 backend, call `egui_wgpu::Renderer::new(&device, TextureFormat::Bgra8Unorm, RendererOptions::default())` in a fresh process and time it.
2. Compare 0.36.2 with 0.37.0. Alternatively, run eframe with `RUST_LOG=wgpu_core=trace` and compare the gap between `create_pipeline_layout` and `create_render_pipeline`.

**Measurements** (Intel integrated GPU, Windows 11 26100, wgpu 30.0.1, FXC; median of 7 fresh processes, `Renderer::new` only):

| shader | time |
|---|---|
| 0.37.0 as published | 57 ms |
| `wrap_texel_coord` reduced to `clamp` only (0.36 behavior) | 14 ms |
| same semantics, float math + `select`, two calls per bilinear sample | 26 ms |

In a full eframe app the first frame went from 197 ms to 258 ms (median of 14 runs), and to 215 ms with the patched shader.

**Suggested fix** (same results for all wrap modes): compute the Euclidean modulo in float, since texel coordinates are far below 2^24. Select the wrap mode with `select` instead of branches. Wrapping is per axis, so two calls are enough for all four bilinear taps:

```wgsl
fn euclid_mod(a: vec2<f32>, n: vec2<f32>) -> vec2<f32> {
    var r = a - n * floor(a / n);
    r = select(r, r - n, r >= n);              // GPU division may be off by one ulp
    r = select(r, r + n, r < vec2<f32>(0.0));
    return r;
}

fn wrap_texel_coord(coord: vec2<i32>, texture_size: vec2<i32>) -> vec2<i32> {
    let wrap_mode = r_tex_flags[0] >> 1u;
    let c = vec2<f32>(coord);
    let n = vec2<f32>(texture_size);
    let clamped = clamp(c, vec2<f32>(0.0), n - 1.0);
    let repeated = euclid_mod(c, n);
    let period = 2.0 * n;
    let phase = euclid_mod(c, period);
    let mirrored = min(phase, period - 1.0 - phase);
    let wrapped = select(repeated, mirrored, wrap_mode == WRAP_MODE_MIRRORED_REPEAT);
    return vec2<i32>(select(clamped, wrapped, wrap_mode != WRAP_MODE_CLAMP_TO_EDGE));
}

// in sample_texture:
let p0 = wrap_texel_coord(pixel_floor, texture_size);
let p1 = wrap_texel_coord(pixel_floor + vec2<i32>(1, 1), texture_size);
let p00 = p0;
let p10 = vec2<i32>(p1.x, p0.y);
let p01 = vec2<i32>(p0.x, p1.y);
let p11 = p1;
```

A pipeline-overridable constant for `predictable_texture_filtering` would avoid compiling this path at all for apps that never enable it.

**Desktop:** Windows 11 Pro 10.0.26300, Intel(R) Graphics (driver 32.0.101.8626), egui/eframe/egui-wgpu 0.37.0, wgpu 30.0.1, DX12 backend.
