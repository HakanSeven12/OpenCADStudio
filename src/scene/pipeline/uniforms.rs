use crate::scene::view::camera::Camera;
use iced::Rectangle;

#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Uniforms {
    pub viewport_size: [f32; 2],
    /// World units per screen pixel at the current zoom. Used by the
    /// hatch shader to substitute solid fill when pattern line spacing
    /// falls below ~2 px (Phase 3.3 LOD).
    pub world_per_pixel: f32,
    /// LWDISPLAY toggle (1.0 = show lineweights, 0.0 = force 1 px).
    /// Read by the wire shader so the toggle does not require a retessellate.
    pub lwdisplay_enable: f32,
    /// 1.0 → mesh fragment shader replaces the interpolated vertex
    /// normal with `normalize(cross(dpdx(pos), dpdy(pos)))` so each
    /// triangle gets a uniform shade (FlatShaded mode); 0.0 → keeps the
    /// per-vertex normal interpolation (GouraudShaded-style).
    pub flat_shade: f32,
    /// Transparency-display toggle (1.0 = honour entity transparency,
    /// 0.0 = force opaque). Read by the wire shader so the toggle does not
    /// require a retessellate.
    pub transparency_enable: f32,
    /// Per-viewport PSLTSCALE factor. Kept in the shared frame uniform so
    /// zooming inside MSPACE changes one scalar instead of re-tessellating and
    /// re-uploading every dashed wire.
    pub linetype_scale: f32,
    /// Pads the struct to the uniform alignment required by wgpu.
    pub _pad: f32,

    // ── Relative-to-eye (double-single) additions ───────────────────────────
    // Appended at the end so existing field offsets are unchanged; shaders that
    // still read only the legacy fields keep working. Pipelines migrate to RTE
    // one at a time. `view_rot` is the rotation-only view-projection; vertices
    // pre-subtract the eye (via `eye_high`/`eye_low`, two f32 emulating f64) so
    // the large eye translation never enters the f32 matrix → no large-coord
    // jitter.
    pub view_rot: glam::Mat4,
    pub eye_high: [f32; 3],
    pub _pad_eh: f32,
    pub eye_low: [f32; 3],
    pub _pad_el: f32,

    // Up to four native AcDbLight/Sun sources. Positions are uploaded relative
    // to the current eye, preserving large-coordinate precision without
    // changing mesh buffers.
    /// xyz = eye-relative source position, w = light type (1 distant, 2 point,
    /// 3 spot).
    pub light_position_type: [[f32; 4]; 4],
    /// xyz = source-to-target direction, w = intensity.
    pub light_direction_intensity: [[f32; 4]; 4],
    /// rgb = source colour, w = cosine of hotspot angle.
    pub light_color_hotspot: [[f32; 4]; 4],
    /// x = attenuation mode, y/z = start/end limits, w = cosine of falloff.
    pub light_attenuation: [[f32; 4]; 4],
    /// x = active light count, yzw = per-viewport ambient-light RGB.
    pub lighting: [f32; 4],
    /// x = viewport face-colour mode, y = viewport face opacity,
    /// z = highlight intensity for faces without an attached material,
    /// w = visual-style brightness.
    pub visual_style: [f32; 4],
    /// Viewport visual-style mono/tint colour.
    pub visual_style_color: [f32; 4],
    /// RGB background selected by the viewport; alpha is one when a drawing
    /// background object, rather than the application canvas, supplies it.
    pub viewport_background: [f32; 4],
    /// x/y = normalized linear brightness/contrast in the documented
    /// -10..10 viewport range; z/w are reserved for exposure controls.
    pub view_tone: [f32; 4],
    pub background_top: [f32; 4],
    pub background_middle: [f32; 4],
    pub background_bottom: [f32; 4],
    pub background_aux0: [f32; 4],
    pub background_aux1: [f32; 4],
    /// x = mode, y = horizon, z = height, w = rotation in radians.
    pub background_params: [f32; 4],
    /// x = fit, y = keep aspect, z = tile, w = decoded image aspect.
    pub background_image_params: [f32; 4],
    /// xy = image offset, zw = image scale.
    pub background_image_transform: [f32; 4],
    /// x = environment enabled, y = rotation, z = diffuse strength,
    /// w = reflection strength.
    pub environment_params: [f32; 4],
    /// View-to-world rotation used to derive environment lookup directions.
    pub environment_view: glam::Mat4,
    /// Eye-relative light projection for the first shadow-casting source.
    pub shadow_view_proj: glam::Mat4,
    /// x = enabled, y = depth bias, z = softness, w = light index.
    pub shadow_params: [f32; 4],
    /// RGB = document render-environment fog colour.
    pub fog_color: [f32; 4],
    /// x = enabled, y = affect background, z/w = near/far density.
    pub fog_params: [f32; 4],
    /// x/y = near/far eye distance used to interpolate the density.
    pub fog_distances: [f32; 4],
    /// Eight normalized vertical-candela samples for each photometric light.
    pub light_web_profile_a: [[f32; 4]; 4],
    pub light_web_profile_b: [[f32; 4]; 4],
    /// xyz = web rotation, w = profile enabled.
    pub light_web_rotation: [[f32; 4]; 4],
}

impl Uniforms {
    pub fn new(camera: &Camera, bounds: Rectangle, lwdisplay_enable: bool) -> Self {
        let half_h = camera.ortho_size();
        let world_per_pixel = if bounds.height > 0.0 {
            (2.0 * half_h) / bounds.height
        } else {
            0.0
        };
        let (eye_high, eye_low) = camera.eye_high_low();
        Self {
            viewport_size: [bounds.width, bounds.height],
            world_per_pixel,
            lwdisplay_enable: if lwdisplay_enable { 1.0 } else { 0.0 },
            flat_shade: 0.0,
            transparency_enable: 1.0,
            linetype_scale: 1.0,
            _pad: 0.0,
            view_rot: camera.view_proj_rte(bounds),
            eye_high,
            _pad_eh: 0.0,
            eye_low,
            _pad_el: 0.0,
            light_position_type: [[0.0; 4]; 4],
            light_direction_intensity: [[0.0; 4]; 4],
            light_color_hotspot: [[0.0; 4]; 4],
            light_attenuation: [[0.0; 4]; 4],
            lighting: [0.0, 0.18, 0.18, 0.18],
            visual_style: [1.0, 1.0, -1.0, 0.0],
            visual_style_color: [1.0, 1.0, 1.0, 1.0],
            viewport_background: [0.0, 0.0, 0.0, 0.0],
            view_tone: [0.0; 4],
            background_top: [0.0; 4],
            background_middle: [0.0; 4],
            background_bottom: [0.0; 4],
            background_aux0: [0.0; 4],
            background_aux1: [0.0; 4],
            background_params: [0.0; 4],
            background_image_params: [0.0; 4],
            background_image_transform: [0.0, 0.0, 1.0, 1.0],
            environment_params: [0.0, 0.0, 0.25, 0.35],
            environment_view: glam::Mat4::IDENTITY,
            shadow_view_proj: glam::Mat4::IDENTITY,
            shadow_params: [0.0, 0.0015, 1.0, 0.0],
            fog_color: [0.0, 0.0, 0.0, 1.0],
            fog_params: [0.0; 4],
            fog_distances: [0.0, 1.0, 0.0, 0.0],
            light_web_profile_a: [[1.0; 4]; 4],
            light_web_profile_b: [[1.0; 4]; 4],
            light_web_rotation: [[0.0; 4]; 4],
        }
    }
}
