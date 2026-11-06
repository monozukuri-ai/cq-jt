use super::{ExportError, ExportOptions};
use crate::mesh::Mesh;
use std::collections::HashMap;

pub(super) struct Prepared {
    pub positions: Vec<[f32; 3]>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub indices: Vec<u32>,
    pub bounds: [f32; 6],
    pub error: f64,
    pub degenerates: u64,
}
pub(super) fn prepare(
    mesh: &Mesh,
    units: f64,
    options: &ExportOptions,
    remaining_vertices: u64,
    remaining_bytes: u64,
) -> Result<Prepared, ExportError> {
    let fail = |code, message| ExportError::new(code, message, Some(&mesh.source));
    let index_bytes = mesh.triangles.len() as u128 * 12;
    if index_bytes > remaining_bytes as u128 {
        return Err(fail(
            "export_byte_limit",
            "index buffer exceeds remaining output budget",
        ));
    }
    if mesh
        .normal_indices
        .as_ref()
        .is_some_and(|v| v.len() != mesh.triangles.len())
        || (mesh.normal_indices.is_none() && !mesh.normals.is_empty())
    {
        return Err(fail(
            "invalid_normal_binding",
            "normal indices and triangle records do not agree",
        ));
    }
    let mut positions = Vec::new();
    let mut normals = mesh.normal_indices.as_ref().map(|_| Vec::new());
    let mut indices = Vec::new();
    let mut pairs = HashMap::new();
    let mut bounds: Option<[f32; 6]> = None;
    let mut error = 0f64;
    let mut degenerates = 0;
    for (triangle_index, triangle) in mesh.triangles.iter().enumerate() {
        let mut original = [[0.; 3]; 3];
        let mut rounded = [[0.; 3]; 3];
        for corner in 0..3 {
            let position_index = triangle[corner];
            let normal_index = mesh
                .normal_indices
                .as_ref()
                .map(|v| v[triangle_index][corner]);
            let p = mesh.positions.get(position_index as usize).ok_or_else(|| {
                fail(
                    "invalid_position_index",
                    "position index is outside its buffer",
                )
            })?;
            original[corner] = p.map(|v| v * units);
            let key = (position_index, normal_index);
            let index = if let Some(&index) = pairs.get(&key) {
                index
            } else {
                if positions.len() as u64 >= remaining_vertices
                    || positions.len() >= u32::MAX as usize
                {
                    return Err(fail(
                        "export_vertex_limit",
                        "unified vertex count exceeds its limit",
                    ));
                }
                if (positions.len() as u128 + 1) * if normals.is_some() { 24 } else { 12 }
                    + index_bytes
                    > remaining_bytes as u128
                {
                    return Err(fail(
                        "export_byte_limit",
                        "unified vertex buffers exceed remaining output budget",
                    ));
                }
                let mut v = [0f32; 3];
                for (i, item) in v.iter_mut().enumerate() {
                    let input = original[corner][i];
                    *item = input as f32;
                    if !input.is_finite() || !item.is_finite() {
                        return Err(fail(
                            "position_out_of_range",
                            "position cannot be represented as finite glTF f32",
                        ));
                    }
                    let delta = (input - f64::from(*item)).abs();
                    error = error.max(delta);
                    if delta > options.max_position_error_meters {
                        return Err(fail(
                            "position_precision_loss",
                            "position rounding exceeds max_position_error_meters",
                        ));
                    }
                }
                if let Some(normal_index) = normal_index {
                    let n = mesh.normals.get(normal_index as usize).ok_or_else(|| {
                        fail("invalid_normal_index", "normal index is outside its buffer")
                    })?;
                    let length = n[0].hypot(n[1]).hypot(n[2]);
                    if !length.is_finite() || length == 0. || n.iter().any(|x| !x.is_finite()) {
                        return Err(fail("invalid_normal", "normal must be finite and nonzero"));
                    }
                    normals
                        .as_mut()
                        .unwrap()
                        .push(n.map(|x| (x / length) as f32));
                }
                let b = bounds.get_or_insert([v[0], v[1], v[2], v[0], v[1], v[2]]);
                for j in 0..3 {
                    b[j] = b[j].min(v[j]);
                    b[j + 3] = b[j + 3].max(v[j]);
                }
                let id = positions.len() as u32;
                positions.push(v);
                pairs.insert(key, id);
                id
            };
            rounded[corner] = positions[index as usize].map(f64::from);
            indices.push(index);
        }
        let before = degenerate(original);
        let after = degenerate(rounded);
        if !before && after {
            return Err(fail(
                "triangle_precision_loss",
                "f32 conversion collapses a nondegenerate triangle",
            ));
        }
        if before {
            degenerates += 1;
        }
    }
    Ok(Prepared {
        positions,
        normals,
        indices,
        bounds: bounds
            .ok_or_else(|| fail("empty_geometry", "mesh contains no triangle corners"))?,
        error,
        degenerates,
    })
}
fn degenerate(p: [[f64; 3]; 3]) -> bool {
    let a = std::array::from_fn::<_, 3, _>(|i| p[1][i] - p[0][i]);
    let b = std::array::from_fn::<_, 3, _>(|i| p[2][i] - p[0][i]);
    let la = a[0].hypot(a[1]).hypot(a[2]);
    let lb = b[0].hypot(b[1]).hypot(b[2]);
    if la == 0. || lb == 0. {
        return true;
    }
    let a = a.map(|x| x / la);
    let b = b.map(|x| x / lb);
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
    .iter()
    .all(|&x| x == 0.)
}
