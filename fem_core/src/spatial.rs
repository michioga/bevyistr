use bevy::prelude::*;

/// Axis-aligned bounding box.
///
/// This is the basic primitive shared by every spatial query in
/// [`Bvh`]: ray casts (picking), box queries (box select / clipping), and
/// radius queries (contact candidate search, nearest-surface search) all
/// boil down to AABB tests against the hierarchy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: Vec3,

    pub max: Vec3,
}

impl Aabb {
    /// An AABB that contains nothing. Unioning it with any other AABB
    /// yields that AABB unchanged, so it is the natural starting point when
    /// folding over a set of points or boxes.
    pub fn empty() -> Self {
        Self {
            min: Vec3::splat(f32::MAX),
            max: Vec3::splat(f32::MIN),
        }
    }

    /// A degenerate AABB containing only `point`.
    pub fn from_point(point: Vec3) -> Self {
        Self {
            min: point,
            max: point,
        }
    }

    /// The smallest AABB containing every point in `points`, or `None` if
    /// `points` is empty.
    pub fn from_points(points: &[Vec3]) -> Option<Self> {
        let mut iter = points.iter();
        let mut aabb = Self::from_point(*iter.next()?);

        for point in iter {
            aabb = aabb.expand(*point);
        }

        Some(aabb)
    }

    /// Grows this AABB to also contain `point`.
    pub fn expand(self, point: Vec3) -> Self {
        Self {
            min: self.min.min(point),
            max: self.max.max(point),
        }
    }

    /// The smallest AABB containing both `self` and `other`.
    pub fn union(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn extent(&self) -> Vec3 {
        self.max - self.min
    }

    /// `true` if `point` lies within this AABB, inclusive of the boundary.
    pub fn contains_point(&self, point: Vec3) -> bool {
        point.cmpge(self.min).all() && point.cmple(self.max).all()
    }

    /// `true` if `self` and `other` overlap (touching counts as overlapping).
    pub fn intersects_aabb(&self, other: &Self) -> bool {
        self.min.cmple(other.max).all() && self.max.cmpge(other.min).all()
    }

    /// `true` if the ray from `origin` in `direction` intersects this AABB
    /// at a non-negative distance.
    ///
    /// Uses the standard slab method. `direction` may have zero components
    /// (treated as parallel to that axis via `1/0 = inf`).
    pub fn intersects_ray(&self, origin: Vec3, direction: Vec3) -> bool {
        let inv_dir = Vec3::new(1.0 / direction.x, 1.0 / direction.y, 1.0 / direction.z);

        let t1 = (self.min - origin) * inv_dir;
        let t2 = (self.max - origin) * inv_dir;

        let t_min = t1.min(t2);
        let t_max = t1.max(t2);

        let enter = t_min.x.max(t_min.y).max(t_min.z);
        let exit = t_max.x.min(t_max.y).min(t_max.z);

        exit >= enter.max(0.0)
    }

    /// Squared distance from `point` to the nearest point on this AABB (`0`
    /// if `point` is inside).
    pub fn distance_squared_to_point(&self, point: Vec3) -> f32 {
        let clamped = point.clamp(self.min, self.max);

        (point - clamped).length_squared()
    }

    /// `true` if any point of this AABB is within `radius` of `point`.
    pub fn within_radius(&self, point: Vec3, radius: f32) -> bool {
        self.distance_squared_to_point(point) <= radius * radius
    }

    /// Grows this AABB outward by `radius` along every axis.
    ///
    /// Used for ray queries against point/line primitives (nodes, edges),
    /// whose AABBs are degenerate or near-degenerate: a zero-width AABB
    /// would only ever be hit by a ray passing through it exactly, whereas
    /// picking wants "within `radius` of the ray" (a cylinder around the
    /// ray). Expanding the AABB by `radius` before the slab test is the
    /// standard Minkowski-sum approximation of that cylinder test.
    pub fn expanded(&self, radius: f32) -> Self {
        Self {
            min: self.min - Vec3::splat(radius),
            max: self.max + Vec3::splat(radius),
        }
    }
}

/// Maximum number of primitives stored in a single BVH leaf before it is
/// split further.
const LEAF_SIZE: u32 = 4;

#[derive(Debug, Clone, Copy)]
struct BvhNode {
    bounds: Aabb,

    /// For interior nodes, the index of the left child. The right child is
    /// always `left_first + 1`, since both children of a node are always
    /// allocated as a consecutive pair during [`Bvh::subdivide`]. For
    /// leaves, the start index into [`Bvh::primitives`].
    left_first: u32,

    /// `0` for interior nodes; the number of primitives for leaves.
    count: u32,
}

/// A median-split bounding volume hierarchy over a fixed set of [`Aabb`]s.
///
/// This is the "optimizable structure" CLAUDE.md calls for ahead of a real
/// GPU-accelerated BVH/octree: it turns the O(n) linear scans previously
/// used for picking and contact candidate search into roughly O(log n + k)
/// queries, while staying simple enough to rebuild from scratch whenever a
/// mesh's topology cache is rebuilt (no incremental update support is
/// needed yet).
///
/// Query results are indices into the `bounds` slice passed to
/// [`Bvh::build`], i.e. into whatever primitive list (boundary faces, nodes,
/// edges, ...) those bounds were derived from, in the same order. Each query
/// is a broad-phase filter: callers performing exact geometric tests (e.g.
/// ray-triangle intersection) should still do so on the returned candidates.
#[derive(Debug, Clone, Default)]
pub struct Bvh {
    nodes: Vec<BvhNode>,

    primitives: Vec<u32>,
}

impl Bvh {
    /// Builds a BVH over `bounds`.
    pub fn build(bounds: &[Aabb]) -> Self {
        let count = bounds.len();

        if count == 0 {
            return Self::default();
        }

        let centroids: Vec<Vec3> = bounds.iter().map(Aabb::center).collect();
        let mut primitives: Vec<u32> = (0..count as u32).collect();
        let mut nodes = Vec::with_capacity(count * 2);

        nodes.push(BvhNode {
            bounds: Aabb::empty(),
            left_first: 0,
            count: count as u32,
        });

        Self::update_bounds(&mut nodes, 0, bounds, &primitives);
        Self::subdivide(&mut nodes, &mut primitives, bounds, &centroids, 0);

        Self { nodes, primitives }
    }

    /// `true` if this BVH covers zero primitives (e.g. an empty mesh).
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    fn update_bounds(nodes: &mut [BvhNode], node_index: usize, bounds: &[Aabb], primitives: &[u32]) {
        let (first, count) = {
            let node = &nodes[node_index];

            (node.left_first as usize, node.count as usize)
        };

        let mut aabb = Aabb::empty();

        for &primitive in &primitives[first..first + count] {
            aabb = aabb.union(bounds[primitive as usize]);
        }

        nodes[node_index].bounds = aabb;
    }

    /// Recursively splits the node at `node_index` on the longest axis of
    /// its bounds, partitioning its primitive range in place by centroid.
    ///
    /// Stops when a node holds `LEAF_SIZE` or fewer primitives, or when the
    /// split would be degenerate (all centroids fall on one side, e.g.
    /// several coincident primitives), leaving it as a leaf.
    fn subdivide(
        nodes: &mut Vec<BvhNode>,
        primitives: &mut [u32],
        bounds: &[Aabb],
        centroids: &[Vec3],
        node_index: usize,
    ) {
        let (first, count, node_bounds) = {
            let node = &nodes[node_index];

            (node.left_first as usize, node.count as usize, node.bounds)
        };

        if count as u32 <= LEAF_SIZE {
            return;
        }

        let extent = node_bounds.extent();
        let axis = if extent.x >= extent.y && extent.x >= extent.z {
            0
        } else if extent.y >= extent.z {
            1
        } else {
            2
        };
        let split = component(node_bounds.center(), axis);

        let mut i = first;
        let mut j = first + count;

        while i < j {
            if component(centroids[primitives[i] as usize], axis) < split {
                i += 1;
            } else {
                j -= 1;
                primitives.swap(i, j);
            }
        }

        let left_count = i - first;

        if left_count == 0 || left_count == count {
            return;
        }

        let left_index = nodes.len();
        let right_index = left_index + 1;

        nodes.push(BvhNode {
            bounds: Aabb::empty(),
            left_first: first as u32,
            count: left_count as u32,
        });
        nodes.push(BvhNode {
            bounds: Aabb::empty(),
            left_first: (first + left_count) as u32,
            count: (count - left_count) as u32,
        });

        nodes[node_index].left_first = left_index as u32;
        nodes[node_index].count = 0;

        Self::update_bounds(nodes, left_index, bounds, primitives);
        Self::update_bounds(nodes, right_index, bounds, primitives);

        Self::subdivide(nodes, primitives, bounds, centroids, left_index);
        Self::subdivide(nodes, primitives, bounds, centroids, right_index);
    }

    /// Indices of primitives whose AABB intersects the given ray.
    pub fn query_ray(&self, origin: Vec3, direction: Vec3) -> Vec<u32> {
        let mut out = Vec::new();

        if !self.is_empty() {
            self.query_ray_recursive(0, origin, direction, &mut out);
        }

        out
    }

    fn query_ray_recursive(&self, node_index: usize, origin: Vec3, direction: Vec3, out: &mut Vec<u32>) {
        let node = &self.nodes[node_index];

        if !node.bounds.intersects_ray(origin, direction) {
            return;
        }

        if node.count > 0 {
            self.push_leaf(node, out);
        } else {
            self.query_ray_recursive(node.left_first as usize, origin, direction, out);
            self.query_ray_recursive(node.left_first as usize + 1, origin, direction, out);
        }
    }

    /// Indices of primitives within `radius` of the given ray (a cylinder
    /// test, via [`Aabb::expanded`]).
    ///
    /// Intended for node/edge picking, where primitives have degenerate or
    /// near-degenerate AABBs that an exact (zero-radius) [`Bvh::query_ray`]
    /// would essentially never hit.
    pub fn query_ray_with_radius(&self, origin: Vec3, direction: Vec3, radius: f32) -> Vec<u32> {
        let mut out = Vec::new();

        if !self.is_empty() {
            self.query_ray_with_radius_recursive(0, origin, direction, radius, &mut out);
        }

        out
    }

    fn query_ray_with_radius_recursive(
        &self,
        node_index: usize,
        origin: Vec3,
        direction: Vec3,
        radius: f32,
        out: &mut Vec<u32>,
    ) {
        let node = &self.nodes[node_index];

        if !node.bounds.expanded(radius).intersects_ray(origin, direction) {
            return;
        }

        if node.count > 0 {
            self.push_leaf(node, out);
        } else {
            self.query_ray_with_radius_recursive(node.left_first as usize, origin, direction, radius, out);
            self.query_ray_with_radius_recursive(node.left_first as usize + 1, origin, direction, radius, out);
        }
    }

    /// Indices of primitives whose AABB overlaps `query`.
    ///
    /// Intended for box-select and clipping-region queries against
    /// world-space boxes.
    pub fn query_aabb(&self, query: Aabb) -> Vec<u32> {
        let mut out = Vec::new();

        if !self.is_empty() {
            self.query_aabb_recursive(0, &query, &mut out);
        }

        out
    }

    fn query_aabb_recursive(&self, node_index: usize, query: &Aabb, out: &mut Vec<u32>) {
        let node = &self.nodes[node_index];

        if !node.bounds.intersects_aabb(query) {
            return;
        }

        if node.count > 0 {
            self.push_leaf(node, out);
        } else {
            self.query_aabb_recursive(node.left_first as usize, query, out);
            self.query_aabb_recursive(node.left_first as usize + 1, query, out);
        }
    }

    /// Indices of primitives whose AABB is within `radius` of `point`.
    ///
    /// Used for proximity-based contact candidate search and
    /// nearest-surface lookups.
    pub fn query_radius(&self, point: Vec3, radius: f32) -> Vec<u32> {
        let mut out = Vec::new();

        if !self.is_empty() {
            self.query_radius_recursive(0, point, radius, &mut out);
        }

        out
    }

    fn query_radius_recursive(&self, node_index: usize, point: Vec3, radius: f32, out: &mut Vec<u32>) {
        let node = &self.nodes[node_index];

        if !node.bounds.within_radius(point, radius) {
            return;
        }

        if node.count > 0 {
            self.push_leaf(node, out);
        } else {
            self.query_radius_recursive(node.left_first as usize, point, radius, out);
            self.query_radius_recursive(node.left_first as usize + 1, point, radius, out);
        }
    }

    fn push_leaf(&self, node: &BvhNode, out: &mut Vec<u32>) {
        let first = node.left_first as usize;
        let count = node.count as usize;

        out.extend_from_slice(&self.primitives[first..first + count]);
    }
}

fn component(v: Vec3, axis: usize) -> f32 {
    match axis {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}
