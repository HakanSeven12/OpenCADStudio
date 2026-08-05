use acadrust::objects::{
    ObjectType, VisualStyle, VisualStyleProperty, VisualStylePropertyValue,
};
use acadrust::{CadDocument, EntityType, Handle};

#[derive(Clone, Debug)]
pub struct MeshVisualStyle {
    pub full_handle: Option<Handle>,
    pub face_handle: Option<Handle>,
    pub edge_handle: Option<Handle>,
    pub description: String,
    pub style_type: i16,
    pub face_lighting_model: i16,
    pub face_lighting_quality: i16,
    pub face_color_mode: i16,
    pub face_modifier: i32,
    pub face_opacity: f32,
    pub face_specular: f32,
    pub mono_color: Option<[f32; 4]>,
    pub edge_model: i32,
    pub edge_style: i32,
    pub edge_color: Option<[f32; 4]>,
    pub edge_opacity: f32,
    pub edge_width: f32,
    pub edge_overhang: i32,
    pub edge_jitter: i32,
    pub silhouette_color: Option<[f32; 4]>,
    pub silhouette_width: i32,
    pub halo_gap: i32,
    pub isoline_count: i32,
    pub hide_precision: bool,
    pub display_settings: i32,
    pub brightness: f32,
    pub shadow_type: i32,
    pub extended_lighting_model: i16,
    pub internal_use_only: bool,
}

fn core_property(style: &VisualStyle, index: usize) -> Option<&VisualStyleProperty> {
    if style.properties.len() >= 28 {
        return style.properties.get(index);
    }
    if style.properties.len() == 24 {
        let legacy = match index {
            4 => 0,
            5 => 1,
            6 => 2,
            9 => 3,
            10 => 4,
            11 => 5,
            13 => 6,
            14 => 7,
            15 => 8,
            16 => 9,
            17 => 10,
            18 => 11,
            19 => 12,
            20 => 13,
            21 => 14,
            22 => 15,
            23 => 16,
            24 => 17,
            12 => 19,
            25 => 20,
            26 => 21,
            27 => 22,
            _ => return None,
        };
        return style.properties.get(legacy);
    }
    style.properties.get(index)
}

fn property_double(style: &VisualStyle, index: usize, fallback: f64) -> f64 {
    let Some(property) = core_property(style, index) else {
        return fallback;
    };
    match property.value {
        VisualStylePropertyValue::Short(value) => value as f64,
        VisualStylePropertyValue::Long(value) => value as f64,
        VisualStylePropertyValue::Double(value) => value,
        VisualStylePropertyValue::Bool(value) => value as u8 as f64,
        _ => fallback,
    }
}

fn property_long(style: &VisualStyle, index: usize, fallback: i32) -> i32 {
    property_double(style, index, fallback as f64) as i32
}

fn property_bool(style: &VisualStyle, index: usize, fallback: bool) -> bool {
    property_double(style, index, fallback as u8 as f64) != 0.0
}

fn property_color(style: &VisualStyle, index: usize) -> Option<[f32; 4]> {
    let property = core_property(style, index)?;
    if property.enabled == 0 {
        return None;
    }
    let VisualStylePropertyValue::Color(color) = &property.value else {
        return None;
    };
    if matches!(
        color,
        acadrust::types::Color::ByLayer | acadrust::types::Color::ByBlock
    ) {
        return None;
    }
    Some(crate::scene::convert::tess_util::aci_to_rgba(color))
}

impl MeshVisualStyle {
    pub(crate) fn from_dwg(handle: Handle, style: &VisualStyle) -> Self {
        Self {
            full_handle: Some(handle),
            face_handle: None,
            edge_handle: None,
            description: style.description.clone(),
            style_type: style.style_type,
            face_lighting_model: style.face_lighting_model,
            face_lighting_quality: style.face_lighting_quality,
            face_color_mode: style.face_color_mode,
            face_modifier: style.face_modifier,
            face_opacity: property_double(style, 4, 1.0).clamp(0.0, 1.0) as f32,
            face_specular: property_double(style, 5, 0.0).max(0.0) as f32,
            mono_color: property_color(style, 6),
            edge_model: style.edge_model,
            edge_style: style.edge_style,
            edge_color: property_color(style, 15),
            edge_opacity: property_double(style, 16, 1.0).clamp(0.0, 1.0) as f32,
            edge_width: property_double(style, 17, 1.0).max(0.0) as f32,
            edge_overhang: property_long(style, 18, 0),
            edge_jitter: property_long(style, 19, 0),
            silhouette_color: property_color(style, 20),
            silhouette_width: property_long(style, 21, 0),
            halo_gap: property_long(style, 22, 0),
            isoline_count: property_long(style, 23, 0),
            hide_precision: property_bool(style, 24, false),
            display_settings: property_long(style, 25, 0),
            brightness: property_double(style, 26, 0.0) as f32,
            shadow_type: if style.properties.len() == 24 {
                style
                    .properties
                    .get(23)
                    .map_or(0, |property| match property.value {
                        VisualStylePropertyValue::Short(value) => value as i32,
                        VisualStylePropertyValue::Long(value) => value,
                        VisualStylePropertyValue::Double(value) => value as i32,
                        VisualStylePropertyValue::Bool(value) => value as i32,
                        _ => 0,
                    })
            } else {
                property_long(style, 27, 0)
            },
            extended_lighting_model: style.extended_lighting_model,
            internal_use_only: style.internal_use_only,
        }
    }

    fn override_face(&mut self, handle: Handle, style: &VisualStyle) {
        let source = Self::from_dwg(handle, style);
        self.face_handle = Some(handle);
        self.face_lighting_model = source.face_lighting_model;
        self.face_lighting_quality = source.face_lighting_quality;
        self.face_color_mode = source.face_color_mode;
        self.face_modifier = source.face_modifier;
        self.face_opacity = source.face_opacity;
        self.face_specular = source.face_specular;
        self.mono_color = source.mono_color;
        self.brightness = source.brightness;
        self.extended_lighting_model = source.extended_lighting_model;
    }

    fn override_edge(&mut self, handle: Handle, style: &VisualStyle) {
        let source = Self::from_dwg(handle, style);
        self.edge_handle = Some(handle);
        self.edge_model = source.edge_model;
        self.edge_style = source.edge_style;
        self.edge_color = source.edge_color;
        self.edge_opacity = source.edge_opacity;
        self.edge_width = source.edge_width;
        self.edge_overhang = source.edge_overhang;
        self.edge_jitter = source.edge_jitter;
        self.silhouette_color = source.silhouette_color;
        self.silhouette_width = source.silhouette_width;
        self.halo_gap = source.halo_gap;
        self.isoline_count = source.isoline_count;
        self.hide_precision = source.hide_precision;
    }

    pub fn face_visible(&self) -> bool {
        self.face_lighting_model != 0 && self.face_color_mode != 0
    }

    pub fn edges_visible(&self) -> bool {
        self.edge_model != 0
    }

    pub fn face_color(&self, base: [f32; 4]) -> [f32; 4] {
        let mut color = match self.face_color_mode {
            3 | 4 => self.mono_color.unwrap_or(base),
            5 => {
                let tint = self.mono_color.unwrap_or(base);
                [
                    (base[0] + tint[0]) * 0.5,
                    (base[1] + tint[1]) * 0.5,
                    (base[2] + tint[2]) * 0.5,
                    base[3],
                ]
            }
            6 => {
                let luminance = base[0] * 0.2126 + base[1] * 0.7152 + base[2] * 0.0722;
                [
                    base[0] * 0.7 + luminance * 0.3,
                    base[1] * 0.7 + luminance * 0.3,
                    base[2] * 0.7 + luminance * 0.3,
                    base[3],
                ]
            }
            _ => base,
        };
        if self.face_modifier & 1 != 0 {
            color[3] *= self.face_opacity;
        }
        color
    }

    pub fn edge_color(&self, base: [f32; 4]) -> [f32; 4] {
        let mut color = self.edge_color.unwrap_or(base);
        color[3] *= self.edge_opacity;
        color
    }
}

pub(crate) fn resolve_visual_style_handle(
    document: &CadDocument,
    handle: Handle,
) -> Option<MeshVisualStyle> {
    visual_style(document, Some(handle))
        .map(|(handle, style)| MeshVisualStyle::from_dwg(handle, style))
}

fn visual_style(
    document: &CadDocument,
    handle: Option<Handle>,
) -> Option<(Handle, &VisualStyle)> {
    let handle = handle.filter(|handle| handle.is_valid())?;
    match document.objects.get(&handle) {
        Some(ObjectType::VisualStyle(style)) => Some((handle, style)),
        _ => None,
    }
}

pub fn resolve_mesh_visual_style(
    document: &CadDocument,
    entity: &EntityType,
) -> Option<MeshVisualStyle> {
    let common = entity.common();
    let full = visual_style(document, common.full_visual_style_handle);
    let face = visual_style(document, common.face_visual_style_handle);
    let edge = visual_style(document, common.edge_visual_style_handle);
    let mut result = full
        .map(|(handle, style)| MeshVisualStyle::from_dwg(handle, style))
        .or_else(|| {
            face.or(edge)
                .map(|(handle, style)| MeshVisualStyle::from_dwg(handle, style))
        })?;
    if full.is_none() {
        result.full_handle = None;
    }
    if let Some((handle, style)) = face {
        result.override_face(handle, style);
    }
    if let Some((handle, style)) = edge {
        result.override_edge(handle, style);
    }
    Some(result)
}

pub fn apply_mesh_visual_style(
    set: &mut super::mesh_model::MeshLodSet,
    document: &CadDocument,
    entity: &EntityType,
) {
    set.visual_style = resolve_mesh_visual_style(document, entity);
}
