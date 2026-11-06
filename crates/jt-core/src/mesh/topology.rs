//! Bounded dual VFMesh reconstruction, JT 9.5 Rev A Appendix E (Siemens 2010).
use super::codec::{vector, Budget, Cursor, Predictor};
use crate::{ParseError, ParseErrorKind as K};

#[derive(Debug)]
pub(super) struct Symbols {
    pub degrees: [Vec<i32>; 8],
    pub valences: Vec<i32>,
    pub groups: Vec<i32>,
    pub flags: Vec<i32>,
    pub masks: [Vec<i32>; 8],
    pub mask30: Vec<i32>,
    pub mask4: Vec<i32>,
    pub large: Vec<i32>,
    pub splits: Vec<i32>,
    pub positions: Vec<i32>,
    pub hash: u32,
}
impl Symbols {
    pub fn read(r: &mut Cursor<'_>, b: &mut Budget) -> Result<Self, ParseError> {
        let mut degrees = std::array::from_fn(|_| Vec::new());
        for v in &mut degrees {
            *v = vector(r, b, Predictor::None)?;
        }
        let valences = vector(r, b, Predictor::None)?;
        let groups = vector(r, b, Predictor::None)?;
        let flags = vector(r, b, Predictor::Lag1)?;
        let mut masks = std::array::from_fn(|_| Vec::new());
        for v in &mut masks {
            *v = vector(r, b, Predictor::None)?;
        }
        let mask30 = vector(r, b, Predictor::None)?;
        let mask4 = if r.modern {
            vec![0; masks[7].len()]
        } else {
            vector(r, b, Predictor::None)?
        };
        let n = r.count(b.remaining)?;
        b.charge(r, n)?;
        let mut large = Vec::new();
        r.bytes
            .get(r.at..r.at + n * 4)
            .ok_or_else(|| r.invalid("truncated high-degree masks"))?;
        for _ in 0..n {
            large.push(r.i32()?);
        }
        let splits = vector(r, b, Predictor::Lag1)?;
        let positions = vector(r, b, Predictor::None)?;
        let hash = r.u32()?;
        if valences.len() != groups.len() || valences.len() != flags.len() {
            return Err(r.invalid("topology parallel array counts differ"));
        }
        if mask30.len() != masks[7].len() || mask4.len() != masks[7].len() {
            return Err(r.invalid("64-bit mask part counts differ"));
        }
        Ok(Self {
            degrees,
            valences,
            groups,
            flags,
            masks,
            mask30,
            mask4,
            large,
            splits,
            positions,
            hash,
        })
    }
}
#[derive(Debug)]
struct Vertex {
    faces: Vec<i32>,
    group: i32,
    flag: i32,
}
#[derive(Debug)]
struct Face {
    vertices: Vec<i32>,
    mask: Vec<u32>,
    attrs: Vec<u32>,
    empty: usize,
    removed: bool,
}
pub(super) struct Topology {
    pub triangles: Vec<[u32; 3]>,
    pub normal_indices: Vec<[u32; 3]>,
    pub groups: Vec<i32>,
    pub topological_vertices: usize,
    pub attribute_count: usize,
    pub cover_faces: usize,
}
struct Decoder<'a, 'b> {
    s: &'a Symbols,
    r: &'a Cursor<'b>,
    vertices: Vec<Vertex>,
    faces: Vec<Face>,
    active: Vec<usize>,
    degrees: [usize; 8],
    masks: [usize; 8],
    split: usize,
    large: usize,
    attrs: usize,
    edges: u64,
    ops: u64,
    max_edges: u64,
    record_counts: (usize, usize),
}
impl Decoder<'_, '_> {
    fn tick(&mut self, n: usize) -> Result<(), ParseError> {
        self.ops = self.ops.checked_sub(n as u64).ok_or_else(|| {
            self.r.error(
                K::LimitExceeded,
                "topology_work_limit",
                "topology work budget exceeded",
            )
        })?;
        Ok(())
    }
    fn edge(&mut self, n: usize) -> Result<(), ParseError> {
        self.edges = self
            .edges
            .checked_add(n as u64)
            .filter(|&n| n <= self.max_edges)
            .ok_or_else(|| {
                self.r.error(
                    K::LimitExceeded,
                    "topology_edge_limit",
                    "topology adjacency limit exceeded",
                )
            })?;
        Ok(())
    }
    fn vf(&self, v: usize, slot: usize) -> Result<i32, ParseError> {
        self.vertices
            .get(v)
            .and_then(|v| v.faces.get(slot))
            .copied()
            .ok_or_else(|| self.r.invalid("vertex face slot outside ring"))
    }
    fn fv(&self, f: usize, slot: usize) -> Result<i32, ParseError> {
        self.faces
            .get(f)
            .and_then(|f| f.vertices.get(slot))
            .copied()
            .ok_or_else(|| self.r.invalid("face vertex slot outside ring"))
    }
    fn set_vf(&mut self, v: usize, slot: usize, f: i32) -> Result<(), ParseError> {
        if f < -1 || f >= self.faces.len() as i32 {
            return Err(self.r.invalid("invalid dual face reference"));
        }
        let target = self
            .vertices
            .get_mut(v)
            .and_then(|v| v.faces.get_mut(slot))
            .ok_or_else(|| self.r.invalid("invalid vertex face slot"))?;
        if *target != -1 && *target != f {
            return Err(self.r.invalid("conflicting dual face connection"));
        }
        *target = f;
        Ok(())
    }
    fn set_fv(&mut self, f: usize, slot: usize, v: usize) -> Result<(), ParseError> {
        if v >= self.vertices.len() {
            return Err(self.r.invalid("invalid dual vertex reference"));
        }
        let face = self
            .faces
            .get_mut(f)
            .ok_or_else(|| self.r.invalid("invalid dual face reference"))?;
        let target = face
            .vertices
            .get_mut(slot)
            .ok_or_else(|| self.r.invalid("invalid face vertex slot"))?;
        if *target == -1 {
            face.empty -= 1;
            *target = v as i32;
        } else if *target != v as i32 {
            return Err(self.r.invalid("conflicting dual vertex connection"));
        }
        Ok(())
    }
    fn find_v(&mut self, f: usize, v: i32) -> Result<usize, ParseError> {
        let n = self
            .faces
            .get(f)
            .ok_or_else(|| self.r.invalid("missing dual face"))?
            .vertices
            .len();
        self.tick(n)?;
        self.faces[f]
            .vertices
            .iter()
            .position(|&x| x == v)
            .ok_or_else(|| self.r.invalid("dual face does not contain expected vertex"))
    }
    fn find_f(&mut self, v: usize, f: i32) -> Result<usize, ParseError> {
        let n = self
            .vertices
            .get(v)
            .ok_or_else(|| self.r.invalid("missing dual vertex"))?
            .faces
            .len();
        self.tick(n)?;
        self.vertices[v]
            .faces
            .iter()
            .position(|&x| x == f)
            .ok_or_else(|| self.r.invalid("dual vertex does not contain expected face"))
    }
    fn vertex(&mut self) -> Result<usize, ParseError> {
        let id = self.vertices.len();
        let n = *self
            .s
            .valences
            .get(id)
            .ok_or_else(|| self.r.invalid("topology requires another valence"))?;
        if !(2..=65535).contains(&n) {
            return Err(self.r.invalid("invalid dual vertex valence"));
        }
        let flag = self.s.flags[id];
        if flag != 0 && flag != 1 {
            return Err(self.r.invalid("invalid cover face flag"));
        }
        self.edge(n as usize)?;
        self.vertices.push(Vertex {
            faces: vec![-1; n as usize],
            group: self.s.groups[id],
            flag,
        });
        Ok(id)
    }
    fn context(&mut self, v: usize) -> Result<usize, ParseError> {
        let val = self.vertices[v].faces.len();
        self.tick(val)?;
        let mut known = 0;
        let mut sum = 0;
        for &f in &self.vertices[v].faces {
            if f >= 0 {
                known += 1;
                sum += self.faces[f as usize].vertices.len();
            }
        }
        Ok(match val {
            3 => match sum.cmp(&(known * 6)) {
                std::cmp::Ordering::Less => 0,
                std::cmp::Ordering::Equal => 1,
                _ => 2,
            },
            4 => match sum.cmp(&(known * 4)) {
                std::cmp::Ordering::Less => 3,
                std::cmp::Ordering::Equal => 4,
                _ => 5,
            },
            5 => 6,
            _ => 7,
        })
    }
    fn activate_face(&mut self, v: usize, slot: usize) -> Result<usize, ParseError> {
        self.tick(1)?;
        let ctx = self.context(v)?;
        let n = *self.s.degrees[ctx]
            .get(self.degrees[ctx])
            .ok_or_else(|| self.r.invalid("missing face degree symbol"))?;
        self.degrees[ctx] += 1;
        if n == 0 {
            let offset = *self
                .s
                .splits
                .get(self.split)
                .ok_or_else(|| self.r.invalid("missing split face offset"))?;
            let position = *self
                .s
                .positions
                .get(self.split)
                .ok_or_else(|| self.r.invalid("missing split face position"))?;
            self.split += 1;
            if offset <= 0 || offset as usize > self.active.len() || position < 0 {
                return Err(self.r.invalid("invalid split face"));
            }
            let f = self.active[self.active.len() - offset as usize];
            if self.faces[f].removed {
                return Err(self.r.invalid("split refers to removed face"));
            }
            self.set_vf(v, slot, f as i32)?;
            self.connect(v, slot, f, position as usize)?;
            return Ok(f);
        }
        if !(2..=65535).contains(&n) {
            return Err(self.r.invalid("invalid dual face degree"));
        }
        let n = n as usize;
        if self.faces.len() >= self.record_counts.0 {
            return Err(self
                .r
                .invalid("topology has more vertices than declared records"));
        }
        self.edge(n)?;
        let mut mask = vec![0u32; n.div_ceil(32)];
        if n <= 64 {
            let ctx = (n - 2).min(7);
            let at = self.masks[ctx];
            let low = *self.s.masks[ctx]
                .get(at)
                .ok_or_else(|| self.r.invalid("missing attribute mask"))?
                as u32;
            self.masks[ctx] += 1;
            if low >> 30 != 0 {
                return Err(self.r.invalid("attribute mask exceeds 30-bit field"));
            }
            let combined = if ctx == 7 {
                let mid = self.s.mask30[at] as u32;
                let high = self.s.mask4[at] as u32;
                if mid >> 30 != 0 || high >> 4 != 0 {
                    return Err(self.r.invalid("invalid high mask field"));
                }
                low as u64 | ((mid as u64) << 30) | ((high as u64) << 60)
            } else {
                low as u64
            };
            if n < 64 && combined >> n != 0 {
                return Err(self.r.invalid("attribute mask exceeds degree"));
            }
            mask[0] = combined as u32;
            if n > 32 {
                mask[1] = (combined >> 32) as u32;
            }
        } else {
            let end = self.large + mask.len();
            let words = self
                .s
                .large
                .get(self.large..end)
                .ok_or_else(|| self.r.invalid("missing high-degree masks"))?;
            for (m, w) in mask.iter_mut().zip(words) {
                *m = *w as u32;
            }
            self.large = end;
            if n % 32 != 0 && mask[mask.len() - 1] >> (n % 32) != 0 {
                return Err(self.r.invalid("high-degree mask exceeds degree"));
            }
        }
        let count = mask.iter().map(|m| m.count_ones() as usize).sum::<usize>();
        if count > self.record_counts.1.saturating_sub(self.attrs) {
            return Err(self
                .r
                .invalid("topology has more attributes than declared records"));
        }
        let attrs = (self.attrs..self.attrs + count).map(|n| n as u32).collect();
        self.attrs += count;
        let f = self.faces.len();
        self.faces.push(Face {
            vertices: vec![-1; n],
            mask,
            attrs,
            empty: n,
            removed: false,
        });
        self.set_vf(v, slot, f as i32)?;
        self.set_fv(f, 0, v)?;
        self.active.push(f);
        Ok(f)
    }
    fn connect(&mut self, v: usize, slot: usize, f: usize, pos: usize) -> Result<(), ParseError> {
        self.tick(1)?;
        let degree = self
            .faces
            .get(f)
            .ok_or_else(|| self.r.invalid("invalid connect face"))?
            .vertices
            .len();
        if pos >= degree {
            return Err(self.r.invalid("connect position outside ring"));
        }
        self.set_fv(f, pos, v)?;
        let val = self.vertices[v].faces.len();
        let before = self.fv(f, (pos + degree - 1) % degree)?;
        if before >= 0 && self.vf(v, (slot + 1) % val)? == -1 {
            let before = before as usize;
            let p = self.find_f(before, f as i32)?;
            let len = self.vertices[before].faces.len();
            let adjacent = self.vf(before, (p + len - 1) % len)?;
            self.set_vf(v, (slot + 1) % val, adjacent)?;
        }
        let after = self.fv(f, (pos + 1) % degree)?;
        if after >= 0 && self.vf(v, (slot + val - 1) % val)? == -1 {
            let after = after as usize;
            let p = self.find_f(after, f as i32)?;
            let len = self.vertices[after].faces.len();
            let adjacent = self.vf(after, (p + 1) % len)?;
            self.set_vf(v, (slot + val - 1) % val, adjacent)?;
        }
        Ok(())
    }
    fn complete_vertex(&mut self, v: usize, pos: usize) -> Result<(), ParseError> {
        let val = self.vertices[v].faces.len();
        let mut prev = self.vf(v, 0)? as usize;
        let mut p = pos;
        let mut left = 1;
        while left < val {
            let next = self.vf(v, left)?;
            if next < 0 {
                break;
            }
            let deg = self.faces[prev].vertices.len();
            p = (p + deg - 1) % deg;
            let other = self.fv(prev, p)?;
            if other < 0 {
                break;
            }
            let next = next as usize;
            let q = self.find_v(next, other)?;
            let d = self.faces[next].vertices.len();
            let q = (q + d - 1) % d;
            self.connect(v, left, next, q)?;
            prev = next;
            p = q;
            left += 1;
        }
        if left == val {
            return Ok(());
        }
        prev = self.vf(v, 0)? as usize;
        p = pos;
        let mut right = val - 1;
        while right >= left {
            let next = self.vf(v, right)?;
            if next < 0 {
                break;
            }
            p = (p + 1) % self.faces[prev].vertices.len();
            let other = self.fv(prev, p)?;
            if other < 0 {
                break;
            }
            let next = next as usize;
            let q = (self.find_v(next, other)? + 1) % self.faces[next].vertices.len();
            self.connect(v, right, next, q)?;
            prev = next;
            p = q;
            right -= 1;
        }
        for slot in left..=right {
            self.activate_face(v, slot)?;
        }
        Ok(())
    }
    fn next_face(&mut self) -> Result<Option<usize>, ParseError> {
        while self.active.last().is_some_and(|&f| self.faces[f].removed) {
            self.active.pop();
        }
        let mut best = None;
        let mut empty = usize::MAX;
        let mut next = self.active.len();
        while next > self.active.len().saturating_sub(16) {
            next -= 1;
            self.tick(1)?;
            let f = self.active[next];
            if self.faces[f].removed {
                self.active.remove(next);
            } else if self.faces[f].empty < empty {
                empty = self.faces[f].empty;
                best = Some(f);
            }
        }
        Ok(best)
    }
    fn run(&mut self) -> Result<(), ParseError> {
        while self.vertices.len() < self.s.valences.len() {
            let seed = self.vertex()?;
            for slot in 0..self.vertices[seed].faces.len() {
                self.activate_face(seed, slot)?;
            }
            while let Some(f) = self.next_face()? {
                while self.faces[f].empty > 0 {
                    let pos = self.find_v(f, -1)?;
                    let v = self.vertex()?;
                    self.set_vf(v, 0, f as i32)?;
                    self.connect(v, 0, f, pos)?;
                    self.complete_vertex(v, pos)?;
                }
                self.faces[f].removed = true;
            }
        }
        for i in 0..8 {
            if self.degrees[i] != self.s.degrees[i].len() || self.masks[i] != self.s.masks[i].len()
            {
                return Err(self.r.invalid("unused topology degree/mask symbols"));
            }
        }
        if self.large != self.s.large.len()
            || self.split != self.s.splits.len()
            || self.split != self.s.positions.len()
        {
            return Err(self.r.invalid("unused topology split/large mask symbols"));
        }
        Ok(())
    }
}
pub(super) fn decode(
    s: &Symbols,
    r: &Cursor<'_>,
    max_edges: u64,
    max_work: u64,
    max_triangles: u64,
    record_counts: (usize, usize),
) -> Result<Topology, ParseError> {
    let mut d = Decoder {
        s,
        r,
        vertices: Vec::new(),
        faces: Vec::new(),
        active: Vec::new(),
        degrees: [0; 8],
        masks: [0; 8],
        split: 0,
        large: 0,
        attrs: 0,
        edges: 0,
        ops: max_work,
        max_edges,
        record_counts,
    };
    d.run()?;
    let mut triangles = Vec::new();
    let mut normals = Vec::new();
    let mut groups = Vec::new();
    let mut cover = 0;
    for v in 0..d.vertices.len() {
        let vert = &d.vertices[v];
        if vert.flag != 0 || vert.group < 0 {
            cover += 1;
            continue;
        }
        if vert.faces.len() != 3 {
            return Err(r.error(
                K::UnsupportedEncoding,
                "non_triangle_topology",
                "visible dual vertex does not represent a triangle",
            ));
        }
        if triangles.len() as u64 >= max_triangles {
            return Err(r.error(
                K::LimitExceeded,
                "triangle_limit",
                "triangle limit exceeded",
            ));
        }
        let faces = vert.faces.clone();
        let mut tri = [0; 3];
        let mut attrs = [u32::MAX; 3];
        for i in 0..3 {
            let f = usize::try_from(faces[i]).map_err(|_| r.invalid("incomplete dual vertex"))?;
            let pos = d.find_v(f, v as i32)?;
            let face = &d.faces[f];
            tri[i] = f as u32;
            if !face.attrs.is_empty() {
                let mut count = 0;
                for slot in 0..=pos {
                    if face.mask[slot / 32] & (1 << (slot % 32)) != 0 {
                        count += 1;
                    }
                }
                let at = if count == 0 {
                    face.attrs.len() - 1
                } else {
                    count - 1
                };
                attrs[i] = face.attrs[at];
            }
        }
        if tri[0] == tri[1] || tri[1] == tri[2] || tri[2] == tri[0] {
            return Err(r.invalid("triangle repeats a vertex"));
        }
        triangles.push(tri);
        normals.push(attrs);
        groups.push(d.vertices[v].group);
    }
    Ok(Topology {
        triangles,
        normal_indices: normals,
        groups,
        topological_vertices: d.faces.len(),
        attribute_count: d.attrs,
        cover_faces: cover,
    })
}
