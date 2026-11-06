use super::value::*;
use crate::{
    compression::failure, ByteOrder, DecodedDocument, Diagnostic, DiagnosticSeverity, ParseError,
    ParseErrorKind as K, SourceSpan,
};
use std::collections::BTreeSet;

pub(super) struct RawNode {
    pub node: GraphNode,
    pub attrs: Vec<i32>,
    pub children: Vec<i32>,
}
pub(super) struct Parsed {
    pub nodes: Vec<RawNode>,
    pub attributes: Vec<Attribute>,
    pub materials: Vec<Material>,
    pub atoms: Vec<PropertyAtom>,
    pub bindings: Vec<PropertyBinding>,
    pub opaque: Vec<OpaqueObject>,
    pub diagnostics: Vec<Diagnostic>,
    pub complete: bool,
    pub segment_index: usize,
    pub object_ids: BTreeSet<i32>,
}

pub(super) struct Cursor<'a> {
    bytes: &'a [u8],
    pub at: usize,
    order: ByteOrder,
    pub source: SourceSpan,
    modern: bool,
    revision106: bool,
}
impl<'a> Cursor<'a> {
    pub fn new(bytes: &'a [u8], order: ByteOrder, source: SourceSpan, modern: bool) -> Self {
        Self {
            bytes,
            at: 0,
            order,
            source,
            modern,
            revision106: false,
        }
    }
    pub fn error(&self, kind: K, code: &'static str, message: impl Into<String>) -> ParseError {
        let mut span = self.source.clone();
        span.start_offset += self.at as u64;
        span.end_offset = span.start_offset;
        failure(&span, kind, code, message)
    }
    fn take<const N: usize>(&mut self) -> Result<[u8; N], ParseError> {
        let end = self
            .at
            .checked_add(N)
            .filter(|&n| n <= self.bytes.len())
            .ok_or_else(|| {
                self.error(
                    K::InvalidElement,
                    "truncated_structure_data",
                    "typed data extends beyond its element",
                )
            })?;
        let mut out = [0; N];
        out.copy_from_slice(&self.bytes[self.at..end]);
        self.at = end;
        Ok(out)
    }
    pub fn u8(&mut self) -> Result<u8, ParseError> {
        Ok(self.take::<1>()?[0])
    }
    pub fn u16(&mut self) -> Result<u16, ParseError> {
        let b = self.take()?;
        Ok(match self.order {
            ByteOrder::Little => u16::from_le_bytes(b),
            ByteOrder::Big => u16::from_be_bytes(b),
        })
    }
    pub fn i16(&mut self) -> Result<i16, ParseError> {
        Ok(self.u16()? as i16)
    }
    pub fn u32(&mut self) -> Result<u32, ParseError> {
        let b = self.take()?;
        Ok(match self.order {
            ByteOrder::Little => u32::from_le_bytes(b),
            ByteOrder::Big => u32::from_be_bytes(b),
        })
    }
    pub fn i32(&mut self) -> Result<i32, ParseError> {
        Ok(self.u32()? as i32)
    }
    pub fn u64(&mut self) -> Result<u64, ParseError> {
        let b = self.take()?;
        Ok(match self.order {
            ByteOrder::Little => u64::from_le_bytes(b),
            ByteOrder::Big => u64::from_be_bytes(b),
        })
    }
    fn finite(&self, v: f64) -> Result<f64, ParseError> {
        if v.is_finite() {
            Ok(v)
        } else {
            Err(self.error(
                K::InvalidElement,
                "non_finite_value",
                "structure values must be finite",
            ))
        }
    }
    pub fn f32(&mut self) -> Result<f64, ParseError> {
        let bits = self.u32()?;
        self.finite(f64::from(f32::from_bits(bits)))
    }
    fn f64(&mut self) -> Result<f64, ParseError> {
        let bits = self.u64()?;
        self.finite(f64::from_bits(bits))
    }
    fn local_version(&mut self) -> Result<i16, ParseError> {
        if self.modern {
            Ok(i16::from(self.u8()?))
        } else {
            self.i16()
        }
    }
    fn ver(&mut self) -> Result<(), ParseError> {
        let v = self.local_version()?;
        if v == 1 {
            Ok(())
        } else {
            Err(self.error(
                K::UnsupportedEncoding,
                "unsupported_local_version",
                format!("expected local version 1, found {v}"),
            ))
        }
    }
    pub fn done(&self) -> Result<(), ParseError> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(self.error(
                K::UnsupportedEncoding,
                "unparsed_structure_data",
                "typed element has unrecognized trailing fields",
            ))
        }
    }
    fn count(&mut self, width: usize, cap: u64) -> Result<usize, ParseError> {
        let n = self.i32()?;
        if n < 0 {
            return Err(self.error(
                K::InvalidElement,
                "negative_structure_count",
                "negative typed vector count",
            ));
        }
        if n as u64 > cap {
            return Err(self.error(
                K::LimitExceeded,
                "reference_limit_exceeded",
                "typed vector exceeds configured limit",
            ));
        }
        let n = n as usize;
        if n.checked_mul(width)
            .is_none_or(|size| size > self.bytes.len() - self.at)
        {
            return Err(self.error(
                K::InvalidElement,
                "invalid_structure_count",
                "typed vector count exceeds remaining bytes",
            ));
        }
        Ok(n)
    }
    fn ids(&mut self, refs: &mut u64, options: &StructureOptions) -> Result<Vec<i32>, ParseError> {
        let n = self.count(4, u64::from(options.max_references).saturating_sub(*refs))?;
        *refs += n as u64;
        (0..n).map(|_| self.i32()).collect()
    }
    fn floats(&mut self, options: &StructureOptions) -> Result<Vec<f64>, ParseError> {
        let n = self.count(4, u64::from(options.max_references))?;
        (0..n).map(|_| self.f32()).collect()
    }
    fn string(&mut self, options: &StructureOptions) -> Result<String, ParseError> {
        let n = self.count(2, u64::MAX)?;
        if n as u64 > options.max_string_bytes / 2 {
            return Err(self.error(
                K::LimitExceeded,
                "string_limit_exceeded",
                "UTF-16 string exceeds max_string_bytes",
            ));
        }
        let values = (0..n).map(|_| self.u16()).collect::<Result<Vec<_>, _>>()?;
        String::from_utf16(&values).map_err(|_| {
            self.error(
                K::InvalidElement,
                "invalid_utf16",
                "invalid UTF-16 property or filename",
            )
        })
    }
    fn farray<const N: usize>(&mut self) -> Result<[f64; N], ParseError> {
        let mut values = [0.; N];
        for v in &mut values {
            *v = self.f32()?;
        }
        Ok(values)
    }
    fn range(&mut self) -> Result<[i32; 2], ParseError> {
        let r = [self.i32()?, self.i32()?];
        if r[0] < 0 || (r[1] < r[0] && !(self.modern && r[1] == -1)) {
            Err(self.error(
                K::InvalidElement,
                "invalid_count_range",
                "shape count range must be nonnegative and ordered",
            ))
        } else {
            Ok(r)
        }
    }
}

#[derive(Clone, Copy)]
enum Type {
    Node(NodeKind),
    Material,
    Transform,
    String,
    Integer,
    Float,
    Date,
    Vector,
    Late,
}
fn classify(raw: [u8; 16], order: ByteOrder) -> Option<Type> {
    let (a, b, c) = match order {
        ByteOrder::Little => (
            u32::from_le_bytes(raw[..4].try_into().ok()?),
            u16::from_le_bytes(raw[4..6].try_into().ok()?),
            u16::from_le_bytes(raw[6..8].try_into().ok()?),
        ),
        ByteOrder::Big => (
            u32::from_be_bytes(raw[..4].try_into().ok()?),
            u16::from_be_bytes(raw[4..6].try_into().ok()?),
            u16::from_be_bytes(raw[6..8].try_into().ok()?),
        ),
    };
    if (b, c) == (0x2ac8, 0x11d1) && raw[8..] == [0x9b, 0x6b, 0, 0x80, 0xc7, 0xbb, 0x59, 0x97] {
        return Some(match a {
            0x10dd1035 => Type::Node(NodeKind::Base),
            0x10dd101b => Type::Node(NodeKind::Group),
            0x10dd103e => Type::Node(NodeKind::Partition),
            0x10dd102a => Type::Node(NodeKind::Instance),
            0x10dd102c => Type::Node(NodeKind::Lod),
            0x10dd104c => Type::Node(NodeKind::RangeLod),
            0x10dd10f3 => Type::Node(NodeKind::Switch),
            0x10dd1077 => Type::Node(NodeKind::TriStripShape),
            0x10dd1030 => Type::Material,
            0x10dd1083 => Type::Transform,
            0x10dd106e => Type::String,
            0x10dd102b => Type::Integer,
            0x10dd1019 => Type::Float,
            _ => return None,
        });
    }
    if (b, c) == (0x38fb, 0x11d1) && raw[8..] == [0xa5, 6, 0, 0x60, 0x97, 0xbd, 0xc6, 0xe1] {
        return Some(match a {
            0xce357244 => Type::Node(NodeKind::Part),
            0xce357245 => Type::Node(NodeKind::Metadata),
            0xce357246 => Type::Date,
            _ => return None,
        });
    }
    if (a, b, c) == (0xe0b05be5, 0xfbbd, 0x11d1)
        && raw[8..] == [0xa3, 0xa7, 0, 0xaa, 0, 0xd1, 9, 0x54]
    {
        return Some(Type::Late);
    }
    if (a, b, c) == (0x2e7db4be, 0xc71a, 0x4b18)
        && raw[8..] == [0x9d, 7, 0xc7, 0x22, 0x7e, 0x9f, 0xef, 0x76]
    {
        return Some(Type::Vector);
    }
    None
}

pub(super) fn parse(
    document: &DecodedDocument,
    options: &StructureOptions,
) -> Result<Parsed, ParseError> {
    let segment = document
        .segments
        .iter()
        .find(|s| s.guid_raw == document.container.lsg_guid_raw && s.segment_type == 1)
        .expect("container validated the LSG root");
    if segment.elements.first().is_none_or(|e| {
        !matches!(
            classify(e.type_id, document.container.byte_order),
            Some(Type::Node(NodeKind::Partition))
        )
    }) {
        return Err(failure(
            &segment.decoded_source,
            K::InvalidGraph,
            "invalid_structure_root",
            "the first graph element must be a partition node",
        ));
    }
    let mut out = Parsed {
        nodes: vec![],
        attributes: vec![],
        materials: vec![],
        atoms: vec![],
        bindings: vec![],
        opaque: vec![],
        diagnostics: vec![],
        complete: true,
        segment_index: segment.segment_index,
        object_ids: BTreeSet::new(),
    };
    let mut refs = 0u64;
    for (index, element) in segment.elements.iter().enumerate() {
        if element.is_end_marker {
            continue;
        }
        let id = element.object_id.expect("9.5 logical headers have an ID");
        if !out.object_ids.insert(id) {
            return Err(failure(
                &element.source,
                K::InvalidGraph,
                "duplicate_object_id",
                "duplicate object ID within the LSG",
            ));
        }
        let typ = classify(element.type_id, document.container.byte_order);
        let mut cursor = Cursor::new(
            segment.element_bytes(index).expect("framed range"),
            document.container.byte_order,
            element.data_source.clone(),
            document.container.version.major == 10,
        );
        cursor.revision106 =
            document.container.version.major == 10 && document.container.version.minor == 6;
        let result = (|| {
            let typ = typ.ok_or_else(|| {
                cursor.error(
                    K::UnsupportedEncoding,
                    "unsupported_lsg_type",
                    "LSG object type is outside the structure profile",
                )
            })?;
            let base = match typ {
                Type::Node(NodeKind::Base | NodeKind::Instance) => 0,
                Type::Node(NodeKind::TriStripShape) => 2,
                Type::Node(_) => 1,
                Type::Material | Type::Transform => 3,
                Type::Late => 8,
                _ => 5,
            };
            let expected_list = if cursor.revision106
                || matches!(typ, Type::Node(_) | Type::Material | Type::Transform)
            {
                0
            } else {
                1
            };
            if element.list_index != expected_list {
                return Err(cursor.error(
                    K::InvalidElement,
                    "invalid_lsg_list",
                    "known LSG object occurs in the wrong graph/property list",
                ));
            }
            if element.base_type != Some(base) {
                return Err(cursor.error(
                    K::InvalidElement,
                    "invalid_object_base_type",
                    "known type and object base type disagree",
                ));
            }
            match typ {
                Type::Node(kind) => {
                    if out.nodes.len() >= options.max_nodes as usize {
                        return Err(cursor.error(
                            K::LimitExceeded,
                            "node_limit_exceeded",
                            "node count exceeds max_nodes",
                        ));
                    }
                    let node = parse_node(
                        &mut cursor,
                        id,
                        kind,
                        out.nodes.len() as u32,
                        &mut refs,
                        options,
                    )?;
                    cursor.done()?;
                    out.nodes.push(node);
                }
                Type::Material | Type::Transform => {
                    let (mut attr, material) =
                        parse_attribute(&mut cursor, id, matches!(typ, Type::Material))?;
                    cursor.done()?;
                    attr.id = out.attributes.len() as u32;
                    if let Some(mut material) = material {
                        material.id = out.materials.len() as u32;
                        attr.material_id = Some(material.id);
                        out.materials.push(material);
                    }
                    out.attributes.push(attr);
                }
                _ => {
                    let atom = parse_atom(&mut cursor, id, element.type_id, typ, options)?;
                    cursor.done()?;
                    if matches!(atom.value, PropertyValue::Opaque) {
                        out.opaque.push(OpaqueObject {
                            object_id: id,
                            type_id: element.type_id,
                            element_index: index,
                            source: element.source.clone(),
                        });
                        out.diagnostics.push(Diagnostic{code:"opaque_date_property".into(),message:"date-v3 property retained as opaque; date/time extension not interpreted".into(),severity:DiagnosticSeverity::Info,source:Some(element.data_source.clone())});
                    }
                    out.atoms.push(atom);
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            if options.mode != ReadMode::BestEffort || error.kind != K::UnsupportedEncoding {
                return Err(error);
            }
            out.complete = false;
            out.diagnostics.push(Diagnostic {
                code: error.code.into(),
                message: error.message,
                severity: DiagnosticSeverity::Warning,
                source: error.source,
            });
            out.opaque.push(OpaqueObject {
                object_id: id,
                type_id: element.type_id,
                element_index: index,
                source: element.source.clone(),
            });
            if matches!(element.base_type, Some(5 | 8)) {
                out.atoms.push(PropertyAtom {
                    object_id: id,
                    type_id: element.type_id,
                    state_flags: None,
                    value: PropertyValue::Opaque,
                    source: element.data_source.clone(),
                });
            }
        }
    }
    let tail_start =
        (segment.tail_source.start_offset - segment.decoded_source.start_offset) as usize;
    let mut cursor = Cursor::new(
        &segment.bytes()[tail_start..],
        document.container.byte_order,
        segment.tail_source.clone(),
        false, // The property table keeps its U16 version in both generations.
    );
    cursor.ver()?;
    let count = cursor.count(8, u64::from(options.max_references).saturating_sub(refs))?;
    let mut owners = BTreeSet::new();
    for _ in 0..count {
        let owner = cursor.i32()?;
        if !owners.insert(owner) {
            return Err(cursor.error(
                K::InvalidGraph,
                "duplicate_property_owner",
                "duplicate property-table owner",
            ));
        }
        let mut keys = BTreeSet::new();
        loop {
            let start = cursor.at;
            let key = cursor.i32()?;
            if key == 0 {
                break;
            }
            let value = cursor.i32()?;
            refs = refs.saturating_add(2);
            if refs > u64::from(options.max_references) {
                return Err(cursor.error(
                    K::LimitExceeded,
                    "reference_limit_exceeded",
                    "property bindings exceed max_references",
                ));
            }
            if !keys.insert(key) {
                return Err(cursor.error(
                    K::InvalidGraph,
                    "duplicate_property_key",
                    "duplicate property atom key for an owner",
                ));
            }
            let mut source = cursor.source.clone();
            source.start_offset += start as u64;
            source.end_offset = cursor.source.start_offset + cursor.at as u64;
            out.bindings.push(PropertyBinding {
                owner_object_id: owner,
                key_atom_id: key,
                value_atom_id: value,
                key: None,
                source,
            });
        }
    }
    cursor.done()?;
    if out
        .nodes
        .first()
        .is_none_or(|n| n.node.kind != NodeKind::Partition)
    {
        return Err(failure(
            &segment.decoded_source,
            K::InvalidGraph,
            "invalid_structure_root",
            "first LSG graph object must be a supported partition node",
        ));
    }
    Ok(out)
}

fn parse_node(
    c: &mut Cursor<'_>,
    id: i32,
    kind: NodeKind,
    index: u32,
    refs: &mut u64,
    o: &StructureOptions,
) -> Result<RawNode, ParseError> {
    c.ver()?;
    let flags = c.u32()?;
    if flags & !1 != 0 {
        return Err(c.error(
            K::UnsupportedEncoding,
            "unsupported_node_flags",
            "unknown node flags",
        ));
    }
    let attrs = c.ids(refs, o)?;
    let group = !matches!(
        kind,
        NodeKind::Base | NodeKind::Instance | NodeKind::TriStripShape
    );
    let children = if group {
        c.ver()?;
        c.ids(refs, o)?
    } else if kind == NodeKind::Instance {
        c.ver()?;
        *refs += 1;
        if *refs > u64::from(o.max_references) {
            return Err(c.error(
                K::LimitExceeded,
                "reference_limit_exceeded",
                "child references exceed max_references",
            ));
        }
        vec![c.i32()?]
    } else {
        vec![]
    };
    let mut node = GraphNode {
        id: index,
        object_id: id,
        kind,
        flags,
        attribute_ids: vec![],
        child_ids: vec![],
        name: None,
        measurement_units: None,
        file_name: None,
        partition_flags: None,
        reserved: None,
        lod_ranges: vec![],
        lod_center: None,
        selected_child: None,
        shape: None,
        payload_references: vec![],
        source: c.source.clone(),
    };
    match kind {
        NodeKind::Partition => {
            if c.modern && !c.revision106 {
                c.ver()?;
            }
            let flags = c.i32()?;
            if flags & !1 != 0 {
                return Err(c.error(
                    K::UnsupportedEncoding,
                    "unsupported_partition_flags",
                    "unknown partition flags",
                ));
            }
            node.partition_flags = Some(flags);
            if c.revision106 && flags != 1 {
                return Err(c.error(
                    K::UnsupportedEncoding,
                    "unsupported_jt106_partition_bounds",
                    "experimental 10.6 partition requires an explicit untransformed bounding box",
                ));
            }
            node.file_name = Some(c.string(o)?);
            let _bounds = c.farray::<6>()?;
            let _area = c.f32()?;
            c.range()?;
            c.range()?;
            c.range()?;
            // The pinned 10.0/10.3 writers omit the duplicate root box; accept
            // only the exact bounded zero-tail or full-box variants.
            if !c.revision106 && flags & 1 != 0 && (!c.modern || c.bytes.len() - c.at == 24) {
                c.farray::<6>()?;
            }
        }
        NodeKind::Metadata => c.ver()?,
        NodeKind::Part => {
            c.ver()?;
            c.ver()?;
            node.reserved = Some(c.i32()?);
        }
        NodeKind::Lod | NodeKind::RangeLod => {
            c.ver()?;
            if !c.modern {
                let _reserved = c.floats(o)?;
                node.reserved = Some(c.i32()?);
            }
            if kind == NodeKind::RangeLod {
                c.ver()?;
                node.lod_ranges = c.floats(o)?;
                node.lod_center = Some(c.farray()?);
            }
        }
        NodeKind::Switch => {
            c.ver()?;
            let selected = c.i32()?;
            if selected < -1 || selected >= children.len() as i32 {
                return Err(c.error(
                    K::InvalidGraph,
                    "invalid_switch_selection",
                    "Switch child index is outside its child list",
                ));
            }
            node.selected_child = Some(selected);
        }
        NodeKind::TriStripShape => {
            c.ver()?;
            let transformed_bounds = if c.modern { None } else { Some(c.farray()?) };
            let local_bounds = c.farray()?;
            let area = c.f32()?;
            let vertex_count_range = c.range()?;
            let node_count_range = c.range()?;
            let polygon_count_range = c.range()?;
            let size = if c.modern {
                c.u32()?
            } else {
                let value = c.i32()?;
                u32::try_from(value).map_err(|_| {
                    c.error(
                        K::InvalidElement,
                        "invalid_shape_metadata",
                        "negative shape size",
                    )
                })?
            };
            let compression_level = c.f32()?;
            if area < 0.
                || (0..3).any(|i| {
                    transformed_bounds.is_some_and(|b| b[i] > b[i + 3])
                        || local_bounds[i] > local_bounds[i + 3]
                })
            {
                return Err(c.error(
                    K::InvalidElement,
                    "invalid_shape_metadata",
                    "shape bounds or area are invalid",
                ));
            }
            if !(0.0..=1.0).contains(&compression_level) {
                return Err(c.error(
                    K::InvalidElement,
                    "invalid_shape_metadata",
                    "invalid shape size or compression level",
                ));
            }
            let vertex_version = c.local_version()?;
            if !(0..=2).contains(&vertex_version) || (c.modern && vertex_version != 1) {
                return Err(c.error(
                    K::UnsupportedEncoding,
                    "unsupported_vertex_version",
                    "unsupported vertex-shape metadata version",
                ));
            }
            let vertex_binding = c.u64()?;
            let quantization = if c.modern { None } else { Some(c.take::<4>()?) };
            if quantization.is_some_and(|q| q[0] > 24 || q[1] > 13 || q[2] > 24 || q[3] > 24) {
                return Err(c.error(
                    K::InvalidElement,
                    "invalid_quantization",
                    "quantization metadata is out of range",
                ));
            }
            let vertex_binding2 = if !c.modern && vertex_version != 1 {
                Some(c.u64()?)
            } else {
                None
            };
            node.shape = Some(ShapeMetadata {
                transformed_bounds,
                local_bounds,
                area,
                vertex_count_range,
                node_count_range,
                polygon_count_range,
                size,
                compression_level,
                vertex_version,
                vertex_binding,
                quantization,
                vertex_binding2,
            });
        }
        _ => {}
    }
    Ok(RawNode {
        node,
        attrs,
        children,
    })
}

fn parse_attribute(
    c: &mut Cursor<'_>,
    id: i32,
    is_material: bool,
) -> Result<(Attribute, Option<Material>), ParseError> {
    let base_version = c.local_version()?;
    if base_version != 1 && !(c.modern && base_version == 2) {
        return Err(c.error(
            K::UnsupportedEncoding,
            "unsupported_attribute_version",
            "unsupported base attribute version",
        ));
    }
    let state_flags = c.u8()?;
    let inhibit_flags = c.u32()?;
    let field_final_flags = if c.modern { Some(c.u32()?) } else { None };
    if state_flags & !15 != 0 {
        return Err(c.error(
            K::UnsupportedEncoding,
            "unsupported_attribute_flags",
            "unknown attribute state flags",
        ));
    }
    let mut attr = Attribute {
        id: 0,
        object_id: id,
        state_flags,
        inhibit_flags,
        field_final_flags,
        palette_index: None,
        transform: None,
        material_id: None,
        source: c.source.clone(),
    };
    if !is_material {
        if field_final_flags.is_some_and(|v| v != 0) {
            return Err(c.error(
                K::UnsupportedEncoding,
                "unsupported_transform_final_fields",
                "transform has no defined field-final bits",
            ));
        }
        if inhibit_flags != 0 {
            return Err(c.error(
                K::UnsupportedEncoding,
                "unsupported_transform_inhibit",
                "transform has undefined inhibit bits",
            ));
        }
        c.ver()?;
        let mask = c.u16()?;
        let n = mask.count_ones() as usize;
        let remaining = (c.bytes.len() - c.at)
            .checked_sub(if base_version == 2 { 4 } else { 0 })
            .ok_or_else(|| {
                c.error(
                    K::InvalidElement,
                    "truncated_attribute_palette",
                    "missing attribute palette trailer",
                )
            })?;
        let width = if remaining == 8 * n {
            8
        } else if remaining == 4 * n {
            4
        } else {
            return Err(c.error(
                K::InvalidElement,
                "invalid_transform_length",
                "transform storage width does not match its mask",
            ));
        };
        let mut raw = IDENTITY;
        for (i, v) in raw.iter_mut().enumerate() {
            if mask & (0x8000 >> i) != 0 {
                *v = if width == 8 { c.f64()? } else { c.f32()? };
            }
        }
        let mut matrix = IDENTITY;
        for r in 0..4 {
            for col in 0..4 {
                matrix[r * 4 + col] = raw[col * 4 + r];
            }
        }
        if !super::resolve::valid_transform(&matrix) {
            return Err(c.error(
                K::InvalidGraph,
                "invalid_transform",
                "transform must be finite, affine and nonsingular without shear",
            ));
        }
        attr.transform = Some(matrix);
        if base_version == 2 {
            let index = c.u32()?;
            attr.palette_index = Some(index);
            if index != u32::MAX {
                return Err(c.error(
                    K::UnsupportedEncoding,
                    "unsupported_attribute_palette",
                    "attribute palette references are not supported",
                ));
            }
        }
        return Ok((attr, None));
    }
    if inhibit_flags & !0x1ff != 0 || field_final_flags.is_some_and(|v| v & !0x1ff != 0) {
        return Err(c.error(
            K::UnsupportedEncoding,
            "unsupported_material_inhibit",
            "material has undefined inhibit bits",
        ));
    }
    let version = c.local_version()?;
    if !(version == 1 || version == 2 && !c.modern) {
        return Err(c.error(
            K::UnsupportedEncoding,
            "unsupported_material_version",
            "material version must be 1 or 2",
        ));
    }
    let data_flags = c.u16()?;
    let ambient = c.farray()?;
    let diffuse = c.farray()?;
    let specular = c.farray()?;
    let emission = c.farray()?;
    let shininess = c.f32()?;
    let reflectivity = if version == 2 || c.modern {
        Some(c.f32()?)
    } else {
        None
    };
    let bumpiness = if c.modern { Some(c.f32()?) } else { None };
    if base_version == 2 {
        let index = c.u32()?;
        attr.palette_index = Some(index);
        if index != u32::MAX {
            return Err(c.error(
                K::UnsupportedEncoding,
                "unsupported_attribute_palette",
                "attribute palette references are not supported",
            ));
        }
    }
    if ambient
        .iter()
        .chain(&diffuse)
        .chain(&specular)
        .chain(&emission)
        .any(|v| !(0.0..=1.0).contains(v))
        || !(1.0..=128.0).contains(&shininess)
        || reflectivity.is_some_and(|v| !(0.0..=1.0).contains(&v))
        || bumpiness.is_some_and(|v| !(0.0..=1.0).contains(&v))
    {
        return Err(c.error(
            K::InvalidElement,
            "invalid_material_value",
            "material values are outside their defined ranges",
        ));
    }
    let palette_index = attr.palette_index;
    Ok((
        attr,
        Some(Material {
            id: 0,
            object_id: id,
            version,
            state_flags,
            inhibit_flags,
            data_flags,
            ambient,
            diffuse,
            specular,
            emission,
            shininess,
            reflectivity,
            bumpiness,
            field_final_flags,
            palette_index,
            source: c.source.clone(),
        }),
    ))
}

fn parse_atom(
    c: &mut Cursor<'_>,
    id: i32,
    type_id: [u8; 16],
    typ: Type,
    o: &StructureOptions,
) -> Result<PropertyAtom, ParseError> {
    let base = c.local_version()?;
    let state_flags = c.u32()?;
    let version = c.local_version()?;
    if c.modern && matches!(typ, Type::Date) && base == 2 && version == 3 {
        // Observed date-v3 extension is not part of the display profile.
        // Preserve its exact source envelope, without guessing a time zone.
        c.at = c.bytes.len();
        return Ok(PropertyAtom {
            object_id: id,
            type_id,
            state_flags: Some(state_flags),
            value: PropertyValue::Opaque,
            source: c.source.clone(),
        });
    }
    if ![1, 2].contains(&base)
        || ![1, 2].contains(&version)
        || (!c.modern && (base != 1 || version != 1))
    {
        return Err(c.error(
            K::UnsupportedEncoding,
            "unsupported_property_version",
            "unsupported property atom version",
        ));
    }
    let value = match typ {
        Type::String => PropertyValue::String(c.string(o)?.into()),
        Type::Integer => PropertyValue::Integer(c.i32()?),
        Type::Float => PropertyValue::Float(c.f32()?),
        Type::Date => {
            let mut v = [0; 6];
            for x in &mut v {
                *x = c.i16()?;
            }
            PropertyValue::Date(v)
        }
        Type::Vector => PropertyValue::Vector(c.farray()?),
        Type::Late => {
            let segment_guid = c.take()?;
            let segment_type = c.i32()?;
            let payload_object_id = c.i32()?;
            let reserved = if c.modern && version == 2 {
                None
            } else {
                Some(c.i32()?)
            };
            if reserved.is_some_and(|v| v < 1) {
                return Err(c.error(
                    K::InvalidElement,
                    "invalid_late_reference",
                    "late-loaded reference reserved value must be positive",
                ));
            }
            PropertyValue::LateLoaded(LateLoadedReference {
                segment_guid,
                segment_type,
                payload_object_id,
                reserved,
            })
        }
        _ => unreachable!(),
    };
    Ok(PropertyAtom {
        object_id: id,
        type_id,
        state_flags: Some(state_flags),
        value,
        source: c.source.clone(),
    })
}
