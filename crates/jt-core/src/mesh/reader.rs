use super::{
    codec::{vector, Budget, Cursor, Predictor},
    hash, topology, Mesh, MeshOptions,
};
use crate::{ByteOrder, ParseError, ParseErrorKind as K, SourceSpan};

#[derive(Clone, Copy)]
pub(super) enum Layout {
    Legacy95,
    Modern10,
    Rev106,
}

pub(super) fn parse(
    bytes: &[u8],
    order: ByteOrder,
    source: SourceSpan,
    options: &MeshOptions,
    budget: &mut Budget,
    layout: Layout,
    externally_compressed: bool,
) -> Result<Mesh, ParseError> {
    let modern = !matches!(layout, Layout::Legacy95);
    budget.deepest = 0;
    let mut r = Cursor {
        bytes,
        at: 0,
        order,
        source: source.clone(),
        modern,
        array_float_hash: matches!(layout, Layout::Modern10),
        externally_compressed,
    };
    r.version(1)?; // BaseShapeLOD (present in observed 9.5 writer profile).
    r.version(1)?;
    let bindings = r.u64()?;
    if modern {
        let length = r.count(bytes.len() as u64)?;
        // JT 10 embeds a full logical header for the topological LOD object.
        if length != bytes.len() - 15 {
            return Err(r.invalid("embedded LOD element length mismatch"));
        }
        let guid = r.take(16)?;
        let mut expected = [
            0xad, 0xa5, 0x30, 0xf8, 0x4c, 0xbe, 0xbc, 0x4f, 0x9b, 0x5f, 0xb9, 0x26, 0x92, 0x78,
            0xd2, 0xe1,
        ];
        if order == ByteOrder::Big {
            expected[..4].reverse();
            expected[4..6].reverse();
            expected[6..8].reverse();
        }
        if guid != expected || r.u8()? != 9 {
            return Err(r.error(
                K::UnsupportedEncoding,
                "unsupported_embedded_lod_type",
                "unsupported embedded topological LOD type",
            ));
        }
        let _object_id = r.i32()?;
    }
    let topo_version = if modern { u16::from(r.u8()?) } else { r.u16()? };
    if ![1, 2].contains(&topo_version) || (modern && topo_version != 1) {
        return Err(r.error(
            K::UnsupportedEncoding,
            "unsupported_topomesh_version",
            "TopoMesh version must be 1 or 2",
        ));
    }
    let vertex_records_object_id = if modern {
        i64::from(r.u32()?)
    } else {
        i64::from(r.i32()?)
    };
    let rep_version = if modern { u16::from(r.u8()?) } else { r.u16()? };
    if ![1, 2].contains(&rep_version) || (modern && rep_version != 1) {
        return Err(r.error(
            K::UnsupportedEncoding,
            "unsupported_topology_version",
            "topological representation version must be 1 or 2",
        ));
    }
    let symbols = topology::Symbols::read(&mut r, budget)?;
    let mut hash = 0;
    for v in &symbols.degrees {
        hash = hash::words(v, hash);
    }
    hash = hash::words(&symbols.valences, hash);
    hash = hash::words(&symbols.groups, hash);
    hash = hash::shorts(&symbols.flags, hash);
    for v in &symbols.masks {
        hash = hash::words(v, hash);
    }
    hash = hash::words(&symbols.mask30, hash);
    if !modern {
        hash = hash::words(&symbols.mask4, hash);
    }
    hash = hash::words(&symbols.large, hash);
    hash = hash::words(&symbols.splits, hash);
    hash = hash::words(&symbols.positions, hash);
    if hash != symbols.hash {
        return Err(r.invalid(format!(
            "topology hash mismatch: {hash:08x} != {:08x}",
            symbols.hash
        )));
    }
    let vertex_bindings = r.u64()?;
    let quantization: [u8; 4] = r.take(4)?.try_into().unwrap();
    let count = r.count(options.max_vertices)?;
    if vertex_bindings != bindings || bindings & !0x4a != 0 || bindings & 2 == 0 {
        return Err(r.error(
            K::UnsupportedEncoding,
            "unsupported_mesh_bindings",
            format!("unsupported or inconsistent vertex bindings {bindings:x}/{vertex_bindings:x}"),
        ));
    }
    let attributes = if count > 0 {
        r.count(options.max_vertices)?
    } else {
        0
    };
    let visible = symbols
        .flags
        .iter()
        .zip(&symbols.groups)
        .filter(|(&flag, &group)| flag == 0 && group >= 0)
        .count();
    if visible as u64 > options.max_triangles {
        return Err(r.error(
            K::LimitExceeded,
            "triangle_limit",
            "triangle count exceeds limit",
        ));
    }
    let required = count as u128 * 24
        + attributes as u128
            * (if bindings & 8 != 0 { 24 } else { 0 } + if bindings & 0x40 != 0 { 4 } else { 0 })
        + visible as u128 * (16 + if bindings & 8 != 0 { 12 } else { 0 });
    if required > options.max_mesh_bytes as u128 {
        return Err(r.error(
            K::LimitExceeded,
            "mesh_byte_limit",
            "mesh buffer byte limit exceeded",
        ));
    }
    let (positions, quantization_step, coordinate_hash) = if count > 0 {
        coordinates(&mut r, budget, count)?
    } else {
        (Vec::new(), [0.; 3], None)
    };
    let (normals, normal_hash) = if count > 0 && bindings & 8 != 0 {
        normals(&mut r, budget, attributes)?
    } else {
        (Vec::new(), None)
    };
    let vertex_flags = if count > 0 && bindings & 0x40 != 0 {
        let n = r.count(options.max_vertices)?;
        let values = vector(&mut r, budget, Predictor::None)?;
        if n != attributes || values.len() != n {
            return Err(r.invalid("vertex flag count mismatch"));
        }
        values.into_iter().map(|v| v as u32).collect()
    } else {
        Vec::new()
    };
    // The v2 profile appends the auxiliary-record envelope, also observed in all
    // pinned 9.5 files. Preserve only its no-auxiliary variant, never skip a tail.
    if rep_version == 2 {
        r.version(1)?;
        let auxiliary_bindings = r.u64()?;
        if auxiliary_bindings != bindings {
            return Err(r.error(
                K::UnsupportedEncoding,
                "unsupported_auxiliary_bindings",
                "auxiliary vertex fields are not supported",
            ));
        }
    }
    r.version(1)?;
    r.done()?;
    let topo = topology::decode(
        &symbols,
        &r,
        options.max_topology_edges,
        options.max_topology_work,
        options.max_triangles,
        (count, attributes),
    )?;
    if topo.topological_vertices != count || topo.attribute_count != attributes {
        return Err(r.invalid(format!(
            "topology/record counts mismatch: {}/{count} vertices, {}/{attributes} attributes",
            topo.topological_vertices, topo.attribute_count
        )));
    }
    if !normals.is_empty() {
        for &i in topo.normal_indices.iter().flatten() {
            let v = normals
                .get(i as usize)
                .ok_or_else(|| r.invalid("normal index outside attribute records"))?;
            if v.iter().all(|&x| x == 0.) {
                return Err(r.invalid("referenced normal has zero length"));
            }
        }
    }
    let mut bounds = None;
    for p in &positions {
        let b = bounds.get_or_insert([p[0], p[1], p[2], p[0], p[1], p[2]]);
        for j in 0..3 {
            b[j] = b[j].min(p[j]);
            b[j + 3] = b[j + 3].max(p[j]);
        }
    }
    let normal_indices = if normals.is_empty() {
        None
    } else {
        Some(topo.normal_indices)
    };
    Ok(Mesh {
        id: 0,
        segment_index: 0,
        element_index: 0,
        vertex_records_object_id,
        source,
        positions,
        triangles: topo.triangles,
        normals,
        normal_indices,
        face_groups: topo.groups,
        vertex_flags,
        bounds,
        quantization,
        quantization_step,
        topology_hash: symbols.hash,
        coordinate_hash,
        normal_hash,
        cover_faces: topo.cover_faces as u64,
        codec_depth: budget.deepest,
    })
}
fn checked_vector(
    r: &mut Cursor<'_>,
    b: &mut Budget,
    p: Predictor,
    n: usize,
) -> Result<Vec<i32>, ParseError> {
    let v = vector(r, b, p)?;
    if v.len() != n {
        return Err(r.invalid("compressed component count mismatch"));
    }
    Ok(v)
}
fn floats(
    r: &mut Cursor<'_>,
    b: &mut Budget,
    n: usize,
    p: Predictor,
) -> Result<Vec<f64>, ParseError> {
    if r.modern {
        return checked_vector(r, b, p, n)?
            .into_iter()
            .map(|bits| {
                let value = f32::from_bits(bits as u32) as f64;
                if value.is_finite() {
                    Ok(value)
                } else {
                    Err(r.invalid("non-finite compressed float"))
                }
            })
            .collect();
    }
    let exp = checked_vector(r, b, p, n)?;
    let man = checked_vector(r, b, p, n)?;
    let mut out = Vec::with_capacity(n);
    for (e, m) in exp.into_iter().zip(man) {
        let v = f32::from_bits(((e as u32) << 23) | m as u32) as f64;
        if !v.is_finite() {
            return Err(r.invalid("non-finite compressed float"));
        }
        out.push(v);
    }
    Ok(out)
}
type Coordinates = (Vec<[f64; 3]>, [f64; 3], Option<u32>);
fn coordinates(r: &mut Cursor<'_>, b: &mut Budget, n: usize) -> Result<Coordinates, ParseError> {
    if r.count(n as u64)? != n || r.u8()? != 3 {
        return Err(r.invalid("coordinate count/components mismatch"));
    }
    let mut ranges = [[0.; 2]; 3];
    let mut bits = [0; 3];
    for i in 0..3 {
        ranges[i] = [r.f32()?, r.f32()?];
        bits[i] = r.u8()?;
        if bits[i] > 32 || ranges[i][0] > ranges[i][1] {
            return Err(r.invalid("invalid coordinate quantizer"));
        }
    }
    if bits[0] != bits[1] || bits[0] != bits[2] {
        return Err(r.error(
            K::UnsupportedEncoding,
            "nonuniform_quantization_bits",
            "coordinate quantizer widths differ",
        ));
    }
    let mut step = [0.; 3];
    let mut components = Vec::new();
    let mut hash = 0;
    for i in 0..3 {
        let values = if bits[i] == 0 {
            let values = floats(r, b, n, Predictor::Lag1)?;
            if r.array_float_hash {
                hash = hash::words(
                    &values
                        .iter()
                        .map(|&v| (v as f32).to_bits() as i32)
                        .collect::<Vec<_>>(),
                    hash,
                );
            } else {
                for &v in &values {
                    hash = hash::words(&[(v as f32).to_bits() as i32], hash);
                }
            }
            values
        } else {
            let codes = checked_vector(r, b, Predictor::Lag1, n)?;
            hash = hash::words(&codes, hash);
            let max = if bits[i] == 32 {
                u32::MAX
            } else {
                (1u32 << bits[i]) - 1
            };
            step[i] = (ranges[i][1] - ranges[i][0]) / max as f64;
            let mut values = Vec::with_capacity(n);
            for code in codes {
                let code = code as u32;
                if code > max {
                    return Err(r.invalid("coordinate code exceeds quantizer width"));
                }
                values.push(ranges[i][0] + code as f64 * step[i]);
            }
            values
        };
        components.push(values);
    }
    let expected = r.u32()?;
    if hash != expected {
        return Err(r.invalid(format!(
            "coordinate hash mismatch: {hash:08x} != {expected:08x}"
        )));
    }
    Ok((
        (0..n)
            .map(|i| [components[0][i], components[1][i], components[2][i]])
            .collect(),
        step,
        Some(expected),
    ))
}
fn normals(
    r: &mut Cursor<'_>,
    b: &mut Budget,
    n: usize,
) -> Result<(Vec<[f64; 3]>, Option<u32>), ParseError> {
    if r.count(n as u64)? != n || r.u8()? != 3 {
        return Err(r.invalid("normal count/components mismatch"));
    }
    let bits = r.u8()?;
    let mut hash = 0;
    let values = if bits == 0 {
        let mut c = Vec::new();
        for _ in 0..3 {
            let values = floats(r, b, n, Predictor::None)?;
            if r.array_float_hash {
                hash = hash::words(
                    &values
                        .iter()
                        .map(|&v| (v as f32).to_bits() as i32)
                        .collect::<Vec<_>>(),
                    hash,
                );
            } else {
                for &v in &values {
                    hash = hash::words(&[(v as f32).to_bits() as i32], hash);
                }
            }
            c.push(values);
        }
        (0..n)
            .map(|i| [c[0][i], c[1][i], c[2][i]])
            .collect::<Vec<_>>()
    } else {
        if bits > 13 {
            return Err(r.invalid("Deering normal width exceeds 13 bits"));
        }
        let mut c = Vec::new();
        for _ in 0..if r.modern { 1 } else { 4 } {
            let codes = checked_vector(r, b, Predictor::None, n)?;
            hash = hash::words(&codes, hash);
            c.push(codes);
        }
        let mut out = Vec::with_capacity(n);
        for (i, &first) in c[0].iter().enumerate() {
            let (s, o, t, p) = if r.modern {
                let code = first as u32;
                let mask = (1u32 << bits) - 1;
                let width = bits * 2 + 6;
                if width < 32 && code >> width != 0 {
                    return Err(r.invalid("packed Deering normal exceeds field width"));
                }
                (
                    ((code >> (bits * 2 + 3)) & 7) as i32,
                    ((code >> (bits * 2)) & 7) as i32,
                    ((code >> bits) & mask) as i32,
                    (code & mask) as i32,
                )
            } else {
                (first, c[1][i], c[2][i], c[3][i])
            };
            out.push(
                deering(s, o, t, p, bits)
                    .ok_or_else(|| r.invalid("invalid Deering normal code"))?,
            );
        }
        out
    };
    let expected = r.u32()?;
    if hash != expected {
        return Err(r.invalid(format!(
            "normal hash mismatch: {hash:08x} != {expected:08x}"
        )));
    }
    Ok((values, Some(expected)))
}
pub(super) fn deering(s: i32, o: i32, t: i32, p: i32, bits: u8) -> Option<[f64; 3]> {
    let range = 1u32.checked_shl(bits as u32)?;
    if !(0..6).contains(&s)
        || !(0..8).contains(&o)
        || t < 0
        || t as u32 >= range
        || p < 0
        || p as u32 >= range
    {
        return None;
    }
    let t = t + (s & 1);
    let theta = (0.615479709 * ((range as i32 - t) as f64) / range as f64)
        .tan()
        .asin();
    let psi = 0.615479709 * (p as f64) / range as f64;
    let a = theta.cos() * psi.cos();
    let b = psi.sin();
    let c = theta.sin() * psi.cos();
    let mut v = match s {
        0 => [a, b, c],
        1 => [c, b, a],
        2 => [b, c, a],
        3 => [b, a, c],
        4 => [c, a, b],
        5 => [a, c, b],
        _ => return None,
    };
    for (i, x) in v.iter_mut().enumerate() {
        if o & (1 << (2 - i)) == 0 {
            *x = -*x;
        }
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn raw(v: &[i32]) -> Vec<u8> {
        let mut out = (v.len() as i32).to_le_bytes().to_vec();
        out.push(0);
        out.extend((v.len() as i32 * 4).to_le_bytes());
        for v in v {
            out.extend(v.to_le_bytes());
        }
        out
    }
    #[test]
    fn nearest_quantizer_inverse_and_exact_endpoints() {
        let mut data = 4i32.to_le_bytes().to_vec();
        data.push(3);
        for _ in 0..3 {
            data.extend(0f32.to_le_bytes());
            data.extend(3f32.to_le_bytes());
            data.push(2);
        }
        let codes = [0, 1, 2, 3];
        let mut h = 0;
        for _ in 0..3 {
            data.extend(raw(&codes));
            h = hash::words(&codes, h);
        }
        data.extend(h.to_le_bytes());
        let mut r = Cursor {
            bytes: &data,
            at: 0,
            order: ByteOrder::Little,
            source: SourceSpan {
                source_id: "quantized".into(),
                byte_domain: "mesh".into(),
                start_offset: 0,
                end_offset: data.len() as u64,
            },
            modern: false,
            array_float_hash: false,
            externally_compressed: false,
        };
        let (positions, step, _) = coordinates(
            &mut r,
            &mut Budget {
                remaining: 100,
                packets: 100,
                depth: 16,
                deepest: 0,
            },
            4,
        )
        .unwrap();
        assert_eq!(positions, [[0.; 3], [1.; 3], [2.; 3], [3.; 3]]);
        assert_eq!(step, [1.; 3]);
        r.done().unwrap();
    }
    #[test]
    fn deering_axes_octants_and_unit_directions() {
        for (sextant, axis) in [(1, 2), (3, 1), (5, 0)] {
            let v = deering(sextant, 7, 63, 0, 6).unwrap();
            for (i, &x) in v.iter().enumerate() {
                assert!((x - if i == axis { 1. } else { 0. }).abs() < 1e-12);
            }
        }
        let neg = deering(1, 0, 63, 0, 6).unwrap();
        assert!((neg[2] + 1.).abs() < 1e-12);
        for s in 0..6 {
            for o in 0..8 {
                let v = deering(s, o, 24, 13, 6).unwrap();
                assert!((v.iter().map(|x| x * x).sum::<f64>() - 1.).abs() < 1e-12);
            }
        }
        assert!(deering(6, 7, 0, 0, 6).is_none());
        assert!(deering(0, 8, 0, 0, 6).is_none());
        assert!(deering(0, 7, 64, 0, 6).is_none());
    }
}
