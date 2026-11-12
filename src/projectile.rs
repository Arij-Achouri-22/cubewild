//! Resolve each projectile segment against terrain and bodies in distance order.

use crate::mobs::Mobs;
use crate::player::{PLAYER_HW, Player, ray_aabb, raycast_with};
use crate::world::{World, is_solid};
use glam::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Block,
    Player,
    Mob(usize),
}

/// A wall wins ties, including a target standing flush against a door or pane.
pub fn trace(world: &World, mobs: &Mobs, player: &Player, hostile: bool, from: Vec3, step: Vec3) -> Option<(Hit, Vec3)> {
    let len = step.length();
    if !len.is_finite() || len < 1e-6 {
        return None;
    }
    let dir = step / len;
    let block = raycast_with(world, from, dir, len, is_solid).map(|(_, _, t)| (Hit::Block, t));
    let target = if hostile {
        let p = player.pos;
        let lo = p - Vec3::new(PLAYER_HW, 0.0, PLAYER_HW);
        let hi = p + Vec3::new(PLAYER_HW, player.height(), PLAYER_HW);
        ray_aabb(from, dir, lo, hi).filter(|&t| t <= len).map(|t| (Hit::Player, t))
    } else {
        mobs.pick_projectile(from, dir, len).map(|(i, t)| (Hit::Mob(i), t))
    };
    let hit = match (block, target) {
        (Some(b), Some(e)) => {
            if e.1 + 1e-5 < b.1 {
                e
            } else {
                b
            }
        }
        (b, e) => b.or(e)?,
    };
    Some((hit.0, from + dir * hit.1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mobs::Kind;
    use crate::world::*;

    fn wall(block: Block) -> World {
        World::flat(|x, y, z| (x == 2 && (10..=11).contains(&y) && (-1..=1).contains(&z)).then_some(block))
    }

    #[test]
    fn walls_panes_and_closed_doors_shield_both_players_and_mobs() {
        let from = Vec3::new(0.5, 10.7, 0.5);
        let player = Player::new(Vec3::new(3.5, 10.0, 0.5));
        let mut mobs = Mobs::new(1);
        mobs.spawn(Kind::Sheep, player.pos);
        for block in [STONE, PANE, DOOR_BASE] {
            let world = wall(block);
            for hostile in [false, true] {
                let (hit, at) = trace(&world, &mobs, &player, hostile, from, Vec3::X * 5.0).unwrap();
                assert_eq!(hit, Hit::Block, "block {block}, hostile {hostile}");
                assert!((2.0..=2.5).contains(&at.x), "impact is on the wall: {at}");
            }
        }
    }

    #[test]
    fn targets_in_front_of_walls_are_hit_and_open_doors_let_shots_pass() {
        let from = Vec3::new(0.5, 10.7, 0.5);
        let mut player = Player::new(Vec3::new(1.5, 10.0, 0.5));
        let mut mobs = Mobs::new(1);
        mobs.spawn(Kind::Sheep, player.pos);
        let world = wall(STONE);
        assert_eq!(trace(&world, &mobs, &player, true, from, Vec3::X * 5.0).unwrap().0, Hit::Player);
        assert_eq!(trace(&world, &mobs, &player, false, from, Vec3::X * 5.0).unwrap().0, Hit::Mob(0));
        player.pos.x = 3.5;
        mobs.list[0].pos.x = 3.5;
        let world = wall(DOOR_BASE + DOOR_OPEN);
        assert_eq!(trace(&world, &mobs, &player, true, from, Vec3::X * 5.0).unwrap().0, Hit::Player);
        assert_eq!(trace(&world, &mobs, &player, false, from, Vec3::X * 5.0).unwrap().0, Hit::Mob(0));
    }

    #[test]
    fn a_target_outside_the_movement_segment_is_not_hit() {
        let world = World::flat(|_, _, _| None);
        let player = Player::new(Vec3::new(3.5, 10.0, 0.5));
        let mobs = Mobs::new(1);
        let from = Vec3::new(0.5, 10.7, 0.5);
        assert!(trace(&world, &mobs, &player, true, from, Vec3::X).is_none());
        assert!(trace(&world, &mobs, &player, true, from, Vec3::ZERO).is_none());
    }

    #[test]
    fn bodies_flush_against_thin_cover_do_not_take_damage_through_it() {
        let from = Vec3::new(0.5, 10.7, 0.5);
        for block in [PANE, DOOR_BASE] {
            let world = wall(block);
            let front = crate::player::block_bounds(&world, glam::IVec3::new(2, 10, 0)).0.x;
            let player = Player::new(Vec3::new(front + PLAYER_HW, 10.0, 0.5));
            let mut mobs = Mobs::new(1);
            mobs.spawn(Kind::Sheep, Vec3::new(front + 0.32, 10.0, 0.5));
            for hostile in [false, true] {
                assert_eq!(trace(&world, &mobs, &player, hostile, from, Vec3::X * 5.0).unwrap().0, Hit::Block);
            }
        }
    }
}
