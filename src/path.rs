//! Path finding for hunting mobs: A* over the cells a body can stand in, so they come
//! round walls, through doorways and open doors, up steps and down into pits.

use crate::world::*;
use glam::IVec3;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// Reads blocks for a search, remembering the last chunk so most lookups skip the hash map.
pub struct Probe<'a> {
    world: &'a World,
    key: Key,
    data: Option<&'a ChunkData>,
}

impl<'a> Probe<'a> {
    pub fn new(world: &'a World) -> Probe<'a> {
        Probe { world, key: (i32::MIN, i32::MIN), data: None }
    }

    pub fn get(&mut self, x: i32, y: i32, z: i32) -> Block {
        if y < 0 {
            return BEDROCK;
        }
        if y >= CH {
            return AIR;
        }
        let k = (x >> 4, z >> 4);
        if k != self.key {
            self.key = k;
            self.data = self.world.chunks.get(&k).map(|s| &*s.data);
        }
        self.data.map_or(AIR, |d| d.get(x & 15, y, z & 15))
    }
}

/// The mover: how many cells tall, how wide in blocks, and how far it will drop.
#[derive(Clone, Copy)]
pub struct Body {
    pub tall: i32,
    pub wide: f32,
    pub drop: i32,
}

/// True if a body fits through a cell holding `b`. An open door leaves a narrower gap,
/// which the body must fit while walking down the middle of the cell.
fn open_cell(b: Block, wide: f32) -> bool {
    !is_solid(b) || (((is_door(b) && door_open(b)) || (is_trapdoor(b) && trap_open(b))) && wide <= 1.0 - 2.0 * DOOR_THICK)
}

fn passable(get: &mut impl FnMut(i32, i32, i32) -> Block, x: i32, y: i32, z: i32, body: Body) -> bool {
    (0..body.tall).all(|i| open_cell(get(x, y + i, z), body.wide))
}

/// True if something holds up a body whose feet are in this cell (water counts: they swim).
fn floor(get: &mut impl FnMut(i32, i32, i32) -> Block, x: i32, y: i32, z: i32) -> bool {
    let below = get(x, y - 1, z);
    // (not on a door's edge, an open trapdoor, or a fence: that is too high to step onto)
    (is_solid(below) && !is_door(below) && !is_fence(below) && !(is_trapdoor(below) && trap_open(below))) || below == WATER || get(x, y, z) == WATER
}

/// The cell a body at `p` stands in: `p` itself, or the ground up to four cells below it.
pub fn settle(get: &mut impl FnMut(i32, i32, i32) -> Block, p: IVec3, body: Body) -> Option<IVec3> {
    for d in 0..=4 {
        let y = p.y - d;
        if !open_cell(get(p.x, y, p.z), body.wide) {
            return None;
        }
        if floor(get, p.x, y, p.z) {
            return Some(IVec3::new(p.x, y, p.z));
        }
    }
    None
}

/// Where a body standing in `p` ends up after moving one column over: level, down a drop,
/// or up one block. Diagonal moves stay level and do not cut corners.
fn step(get: &mut impl FnMut(i32, i32, i32) -> Block, p: IVec3, dx: i32, dz: i32, body: Body) -> Option<IVec3> {
    let (x, z) = (p.x + dx, p.z + dz);
    if dx != 0 && dz != 0 {
        let clear = passable(get, x, p.y, z, body) && passable(get, x, p.y, p.z, body) && passable(get, p.x, p.y, z, body);
        return (clear && floor(get, x, p.y, z)).then_some(IVec3::new(x, p.y, z));
    }
    if passable(get, x, p.y, z, body) {
        for d in 0..=body.drop {
            let y = p.y - d;
            if d > 0 && !open_cell(get(x, y, z), body.wide) {
                return None;
            }
            if floor(get, x, y, z) {
                return Some(IVec3::new(x, y, z));
            }
        }
        None
    } else {
        // a one-block step: needs headroom to jump and somewhere to land
        let up = open_cell(get(p.x, p.y + body.tall, p.z), body.wide) && passable(get, x, p.y + 1, z, body) && floor(get, x, p.y + 1, z);
        up.then_some(IVec3::new(x, p.y + 1, z))
    }
}

/// True if a body can simply walk the straight line from `from` to `to`.
pub fn walk_line(get: &mut impl FnMut(i32, i32, i32) -> Block, from: IVec3, to: IVec3, body: Body) -> bool {
    let (dx, dz) = (to.x - from.x, to.z - from.z);
    let n = dx.abs().max(dz.abs());
    let mut p = from;
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let (sx, sz) = (from.x + (dx as f32 * t).round() as i32 - p.x, from.z + (dz as f32 * t).round() as i32 - p.z);
        let next = if sx != 0 && sz != 0 {
            step(get, p, sx, sz, body).or_else(|| step(get, p, sx, 0, body).and_then(|q| step(get, q, 0, sz, body)))
        } else {
            step(get, p, sx, sz, body)
        };
        match next {
            Some(q) => p = q,
            None => return false,
        }
    }
    (p.y - to.y).abs() <= 1
}

struct Node {
    p: IVec3,
    g: f32,
    parent: u32,
    closed: bool,
}

/// Reusable A* search state (kept between searches so nothing is allocated per search).
#[derive(Default)]
pub struct Finder {
    nodes: Vec<Node>,
    index: HashMap<(i32, i32, i32), u32, Fx>,
    heap: BinaryHeap<Reverse<(u32, u32)>>,
}

impl Finder {
    /// Writes the cells to walk through from `from` (exclusive) towards `to` into `out`,
    /// expanding at most `budget` cells. Returns true if the path arrives; otherwise it
    /// leads to the reachable cell nearest the goal, or is empty if nothing gets closer.
    pub fn find(
        &mut self,
        get: &mut impl FnMut(i32, i32, i32) -> Block,
        from: IVec3,
        to: IVec3,
        body: Body,
        budget: usize,
        out: &mut Vec<IVec3>,
    ) -> bool {
        out.clear();
        self.nodes.clear();
        self.index.clear();
        self.heap.clear();
        let h = |p: IVec3| {
            let (ax, az) = ((p.x - to.x).abs() as f32, (p.z - to.z).abs() as f32);
            ax.max(az) + 0.414 * ax.min(az) + 0.3 * (p.y - to.y).abs() as f32
        };
        let key = |f: f32| (f * 1024.0) as u32;
        self.nodes.push(Node { p: from, g: 0.0, parent: u32::MAX, closed: false });
        self.index.insert((from.x, from.y, from.z), 0);
        self.heap.push(Reverse((key(h(from)), 0)));
        let (mut best, mut best_h) = (0u32, h(from));
        let mut end = None;
        let mut expanded = 0;
        while let Some(Reverse((_, i))) = self.heap.pop() {
            let node = &mut self.nodes[i as usize];
            if node.closed {
                continue;
            }
            node.closed = true;
            let (p, g) = (node.p, node.g);
            if h(p) < best_h {
                (best, best_h) = (i, h(p));
            }
            if p.x == to.x && p.z == to.z && (p.y - to.y).abs() <= 1 {
                end = Some(i);
                break;
            }
            expanded += 1;
            if expanded > budget {
                break;
            }
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let Some(q) = step(get, p, dx, dz, body) else { continue };
                // climbing costs more than walking, dropping a little more
                let dy = q.y - p.y;
                let cost = if dx != 0 && dz != 0 { 1.414 } else { 1.0 } + if dy > 0 { 0.6 } else { -dy as f32 * 0.25 };
                let g2 = g + cost;
                let j = match self.index.get(&(q.x, q.y, q.z)) {
                    Some(&j) => {
                        let n = &mut self.nodes[j as usize];
                        if n.closed || n.g <= g2 {
                            continue;
                        }
                        n.g = g2;
                        n.parent = i;
                        j
                    }
                    None => {
                        let j = self.nodes.len() as u32;
                        self.nodes.push(Node { p: q, g: g2, parent: i, closed: false });
                        self.index.insert((q.x, q.y, q.z), j);
                        j
                    }
                };
                // weighted a little towards the goal: fewer cells searched
                self.heap.push(Reverse((key(g2 + h(q) * 1.2), j)));
            }
        }
        let reached = end.is_some();
        let mut i = match end {
            Some(i) => i,
            None if best_h + 1.0 < h(from) => best,
            None => return false,
        };
        while self.nodes[i as usize].parent != u32::MAX {
            out.push(self.nodes[i as usize].p);
            i = self.nodes[i as usize].parent;
        }
        out.reverse();
        reached
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZOMBIE: Body = Body { tall: 2, wide: 0.6, drop: 4 };

    /// Flat stone ground with its surface at y = 9, plus the blocks `extra` adds on top.
    fn land(extra: impl Fn(i32, i32, i32) -> Option<Block>) -> impl FnMut(i32, i32, i32) -> Block {
        move |x, y, z| extra(x, y, z).unwrap_or(if y <= 9 { STONE } else { AIR })
    }

    /// A wall along x = 5 (three high) with whatever `gap` puts in the cell at z = 0.
    fn wall(gap: Block) -> impl FnMut(i32, i32, i32) -> Block {
        land(move |x, y, z| (x == 5 && (10..13).contains(&y)).then_some(if z == 0 && y < 12 { gap } else { STONE }))
    }

    fn route(get: &mut impl FnMut(i32, i32, i32) -> Block, from: IVec3, to: IVec3) -> (bool, Vec<IVec3>) {
        let mut out = Vec::new();
        let reached = Finder::default().find(get, from, to, ZOMBIE, 600, &mut out);
        (reached, out)
    }

    #[test]
    fn walks_through_a_doorway() {
        let (a, b) = (IVec3::new(0, 10, 6), IVec3::new(9, 10, 6));
        let mut get = wall(AIR);
        assert!(!walk_line(&mut get, a, b, ZOMBIE), "the wall is in the way");
        let (reached, path) = route(&mut get, a, b);
        assert!(reached);
        assert!(path.contains(&IVec3::new(5, 10, 0)), "goes through the gap");
        assert_eq!(path.last(), Some(&b));
        // in the open the straight line is enough
        assert!(walk_line(&mut land(|_, _, _| None), a, b, ZOMBIE));
    }

    #[test]
    fn doors_stop_them_only_when_closed() {
        let (a, b) = (IVec3::new(0, 10, 0), IVec3::new(9, 10, 0));
        // walls on every side of the far half: the door is the only way in
        let room = |door: Block| {
            land(move |x, y, z| {
                let shell = (10..13).contains(&y) && (5..=12).contains(&x) && (-3..=3).contains(&z) && (x == 5 || x == 12 || z.abs() == 3 || y == 12);
                shell.then_some(if x == 5 && z == 0 && y < 12 { door } else { STONE })
            })
        };
        let closed = DOOR_BASE;
        assert!(!route(&mut room(closed), a, b).0);
        assert!(route(&mut room(closed | DOOR_OPEN), a, b).0);
        // too wide for the gap an open door leaves
        let mut out = Vec::new();
        let wide = Body { wide: 0.9, tall: 1, drop: 4 };
        assert!(!Finder::default().find(&mut room(closed | DOOR_OPEN), a, b, wide, 600, &mut out));
        assert!(Finder::default().find(&mut room(AIR), a, b, wide, 600, &mut out));
    }

    #[test]
    fn climbs_steps_and_drops_into_pits() {
        // a pit three deep at x = 6
        let mut pit = land(|x, y, z| (x == 6 && z == 0 && (7..=9).contains(&y)).then_some(AIR));
        let (a, b) = (IVec3::new(0, 10, 0), IVec3::new(6, 7, 0));
        assert_eq!(settle(&mut pit, IVec3::new(6, 10, 0), ZOMBIE), Some(b));
        let (reached, path) = route(&mut pit, a, b);
        assert!(reached && path.last() == Some(&b));
        // out of the pit is too high to climb, a single step is not
        assert!(!route(&mut pit, b, a).0);
        let mut ledge = land(|x, y, _| (x >= 4 && y == 10).then_some(STONE));
        let (reached, path) = route(&mut ledge, a, IVec3::new(8, 11, 0));
        assert!(reached && path.iter().any(|p| p.y == 11));
        // a two-block wall across the whole map cannot be climbed
        let mut cliff = land(|x, y, _| (x >= 4 && (10..12).contains(&y)).then_some(STONE));
        assert!(!route(&mut cliff, a, IVec3::new(8, 12, 0)).0);
    }
}
