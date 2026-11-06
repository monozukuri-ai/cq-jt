use super::{ColorSpace, ExportError, ExportOptions};
use crate::structure::{EffectiveMaterial, Material};
use crate::SourceSpan;
use serde_json::{json, Value};

pub(super) fn material(
    state: &EffectiveMaterial,
    originals_by_id: &std::collections::BTreeMap<(String, i32), &Material>,
    source_indices: &std::collections::BTreeMap<String, usize>,
    options: &ExportOptions,
    source: &SourceSpan,
) -> Result<(Value, bool), ExportError> {
    let fail = |code, message| ExportError::new(code, message, Some(source));
    let fallback = state.diffuse_rgb.is_none() || state.diffuse_alpha.is_none();
    let default = if fallback {
        options.default_color.ok_or_else(|| {
            fail(
                "missing_diffuse_color",
                "diffuse RGB/alpha is unknown; supply default_color explicitly",
            )
        })?
    } else {
        [0.; 4]
    };
    let rgb = state
        .diffuse_rgb
        .unwrap_or([default[0], default[1], default[2]]);
    let alpha = state.diffuse_alpha.unwrap_or(default[3]);
    let rgb = convert(rgb, options.color_space).ok_or_else(|| {
        fail(
            "invalid_material_color",
            "diffuse RGB must be finite and within 0..1",
        )
    })?;
    if !alpha.is_finite() || !(0. ..=1.).contains(&alpha) {
        return Err(fail(
            "invalid_material_alpha",
            "diffuse alpha must be finite and within 0..1",
        ));
    }
    for color in [state.ambient, state.specular, state.emission]
        .into_iter()
        .flatten()
    {
        if color
            .iter()
            .any(|x| !x.is_finite() || !(0. ..=1.).contains(x))
        {
            return Err(fail(
                "invalid_material_color",
                "material channels must be finite and within 0..1",
            ));
        }
    }
    let emission = state
        .emission
        .map(|v| [v[0], v[1], v[2]])
        .unwrap_or([0.; 3]);
    let emission = convert(emission, options.color_space).unwrap();
    let mut originals = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (field, id) in state.origins.iter().enumerate() {
        let Some(id) = id else {
            continue;
        };
        let source = state.origin_sources[field].as_ref().ok_or_else(|| {
            fail(
                "invalid_material_origin",
                "material origin is missing its source identity",
            )
        })?;
        let key = (source.clone(), *id);
        if !source_indices.contains_key(source) {
            return Err(fail(
                "invalid_material_origin",
                "material origin refers to an unknown source identity",
            ));
        }
        if seen.insert(key.clone()) {
            let m = originals_by_id.get(&key).ok_or_else(|| {
                fail(
                    "invalid_material_origin",
                    "effective material references an unknown original object",
                )
            })?;
            if !m.shininess.is_finite()
                || m.reflectivity.is_some_and(|x| !x.is_finite())
                || m.bumpiness.is_some_and(|x| !x.is_finite())
                || [m.ambient, m.diffuse, m.specular, m.emission]
                    .iter()
                    .flatten()
                    .any(|x| !x.is_finite())
            {
                return Err(fail(
                    "invalid_material_metadata",
                    "original material contains non-finite values",
                ));
            }
            originals.push(json!({"objectId":m.object_id,"sourceIndex":source_indices.get(source),"ambient":m.ambient,"diffuse":m.diffuse,"specular":m.specular,"emission":m.emission,"shininess":m.shininess,"reflectivity":m.reflectivity,"bumpiness":m.bumpiness,"fieldFinalFlags":m.field_final_flags,"paletteIndex":m.palette_index,"dataFlags":m.data_flags}));
        }
    }
    let blend = state.blending.unwrap_or(alpha < 1.);
    Ok((
        json!({"pbrMetallicRoughness":{"baseColorFactor":[rgb[0],rgb[1],rgb[2],alpha],"metallicFactor":0.,"roughnessFactor":1.},"emissiveFactor":emission,"alphaMode":if blend{"BLEND"}else{"OPAQUE"},"doubleSided":false,
        "extras":{"jt":{"ambient":state.ambient,"specular":state.specular,"emission":state.emission,"overrideVertexColors":state.override_vertex_colors,"origins":state.origins,"originalMaterials":originals,"defaultDiffuseUsed":fallback,"inputColorSpace":options.color_space.name()}}}),
        fallback,
    ))
}
pub(super) fn convert(rgb: [f64; 3], space: ColorSpace) -> Option<[f64; 3]> {
    if rgb
        .iter()
        .any(|x| !x.is_finite() || !(0. ..=1.).contains(x))
    {
        return None;
    }
    Some(rgb.map(|x| match space {
        ColorSpace::Linear => x,
        ColorSpace::Srgb => {
            if x <= 0.04045 {
                x / 12.92
            } else {
                ((x + 0.055) / 1.055).powf(2.4)
            }
        }
    }))
}
