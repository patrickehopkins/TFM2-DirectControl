//! Generic click-to-entity resolution for direct control.
//!
//! The render thread publishes only a simulation-space cursor point plus the live camera scale.
//! Entity identity is resolved inside the paced simulation callback so the chosen target belongs
//! to the exact simulation state that will consume the command.
//!
//! Click forgiveness is presentation-only. It expands selection geometry in screen-pixel terms;
//! it never changes entity collision, pathing, attack range, or any simulation geometry.

use mod_api_stable::StableSim;

const CHAMPION_PADDING_PX: u64 = 12;
const TOWER_PADDING_PX: u64 = 28;
const MINION_PADDING_PX: u64 = 5;
const OTHER_OBJECTIVE_PADDING_PX: u64 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Champion,
    Tower,
    Minion,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamRelation {
    Hostile,
    Friendly,
    Any,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityPick {
    pub id: usize,
    pub name: Option<String>,
    pub team: usize,
    pub x: u64,
    pub y: u64,
    pub collision_radius: usize,
    pub effective_pick_radius: u64,
    pub distance_sq: u128,
    pub kind: EntityKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CandidateScore {
    id: usize,
    kind: EntityKind,
    effective_radius: u64,
    distance_sq: u128,
}

fn relation_matches(entity_team: usize, controlled_team: usize, relation: TeamRelation) -> bool {
    match relation {
        TeamRelation::Hostile => entity_team != controlled_team,
        TeamRelation::Friendly => entity_team == controlled_team,
        TeamRelation::Any => true,
    }
}

fn kind_priority(kind: EntityKind) -> u8 {
    match kind {
        EntityKind::Champion => 0,
        EntityKind::Tower | EntityKind::Other => 1,
        EntityKind::Minion => 2,
    }
}

fn pick_padding_px(kind: EntityKind) -> u64 {
    match kind {
        EntityKind::Champion => CHAMPION_PADDING_PX,
        EntityKind::Tower => TOWER_PADDING_PX,
        EntityKind::Minion => MINION_PADDING_PX,
        // StableEntity currently has no first-class Nexus/final-objective classifier.
        // Hostile targetable non-champion/non-tower/non-minion entities therefore get the
        // building/objective tier rather than brittle name matching.
        EntityKind::Other => OTHER_OBJECTIVE_PADDING_PX,
    }
}

fn score_candidate(
    id: usize,
    kind: EntityKind,
    entity_team: usize,
    alive: bool,
    targetable: bool,
    visible: bool,
    x: u64,
    y: u64,
    collision_radius: usize,
    controlled_team: usize,
    relation: TeamRelation,
    require_visible: bool,
    click_x: u64,
    click_y: u64,
    sim_units_per_px: u64,
) -> Option<CandidateScore> {
    if !alive
        || !targetable
        || (require_visible && !visible)
        || !relation_matches(entity_team, controlled_team, relation)
    {
        return None;
    }

    let collision_radius = u64::try_from(collision_radius).unwrap_or(u64::MAX);
    let padding = pick_padding_px(kind).saturating_mul(sim_units_per_px);
    let effective_radius = collision_radius.saturating_add(padding);
    let dx = x.abs_diff(click_x) as u128;
    let dy = y.abs_diff(click_y) as u128;
    let distance_sq = dx * dx + dy * dy;
    let radius_sq = effective_radius as u128 * effective_radius as u128;

    (distance_sq <= radius_sq).then_some(CandidateScore {
        id,
        kind,
        effective_radius,
        distance_sq,
    })
}

fn score_is_better(candidate: CandidateScore, current: CandidateScore) -> bool {
    kind_priority(candidate.kind) < kind_priority(current.kind)
        || (kind_priority(candidate.kind) == kind_priority(current.kind)
            && (candidate.distance_sq < current.distance_sq
                || (candidate.distance_sq == current.distance_sq
                    && (candidate.effective_radius < current.effective_radius
                        || (candidate.effective_radius == current.effective_radius
                            && candidate.id < current.id)))))
}

pub fn pick_entity(
    sim: &StableSim<'_>,
    controlled_team: usize,
    relation: TeamRelation,
    require_visible: bool,
    click_x: u64,
    click_y: u64,
    sim_units_per_px: u64,
) -> Option<EntityPick> {
    let mut best: Option<(CandidateScore, EntityPick)> = None;

    for index in 0..sim.entity_count() {
        let Some(entity) = sim.entity_at(index) else {
            continue;
        };

        let kind = if entity.is_champion() {
            EntityKind::Champion
        } else if entity.is_tower() {
            EntityKind::Tower
        } else if entity.is_minion() {
            EntityKind::Minion
        } else {
            EntityKind::Other
        };

        let (x, y) = entity.pos();
        let collision_radius = entity.radius();
        let Some(score) = score_candidate(
            entity.id(),
            kind,
            entity.team(),
            entity.is_alive(),
            entity.is_targetable(),
            sim.is_visible(controlled_team, entity.id()),
            x,
            y,
            collision_radius,
            controlled_team,
            relation,
            require_visible,
            click_x,
            click_y,
            sim_units_per_px,
        ) else {
            continue;
        };

        let picked = EntityPick {
            id: entity.id(),
            name: entity.name(),
            team: entity.team(),
            x,
            y,
            collision_radius,
            effective_pick_radius: score.effective_radius,
            distance_sq: score.distance_sq,
            kind,
        };

        match &best {
            Some((current_score, _)) if !score_is_better(score, *current_score) => {}
            _ => best = Some((score, picked)),
        }
    }

    best.map(|(_, picked)| picked)
}

pub fn pick_hostile_entity(
    sim: &StableSim<'_>,
    controlled_team: usize,
    click_x: u64,
    click_y: u64,
    sim_units_per_px: u64,
) -> Option<EntityPick> {
    pick_entity(
        sim,
        controlled_team,
        TeamRelation::Hostile,
        true,
        click_x,
        click_y,
        sim_units_per_px,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        pick_padding_px, score_candidate, score_is_better, CandidateScore, EntityKind, TeamRelation,
    };

    fn score(
        id: usize,
        kind: EntityKind,
        distance_sq: u128,
        effective_radius: u64,
    ) -> CandidateScore {
        CandidateScore {
            id,
            kind,
            distance_sq,
            effective_radius,
        }
    }

    #[test]
    fn relation_filter_rejects_wrong_team_dead_untargetable_and_hidden_entities() {
        assert!(score_candidate(
            1,
            EntityKind::Champion,
            0,
            true,
            true,
            true,
            100,
            100,
            10,
            0,
            TeamRelation::Hostile,
            true,
            100,
            100,
            1
        )
        .is_none());
        assert!(score_candidate(
            2,
            EntityKind::Champion,
            1,
            false,
            true,
            true,
            100,
            100,
            10,
            0,
            TeamRelation::Any,
            true,
            100,
            100,
            1
        )
        .is_none());
        assert!(score_candidate(
            3,
            EntityKind::Champion,
            1,
            true,
            false,
            true,
            100,
            100,
            10,
            0,
            TeamRelation::Any,
            true,
            100,
            100,
            1
        )
        .is_none());
        assert!(score_candidate(
            4,
            EntityKind::Champion,
            1,
            true,
            true,
            false,
            100,
            100,
            10,
            0,
            TeamRelation::Hostile,
            true,
            100,
            100,
            1
        )
        .is_none());
    }

    #[test]
    fn visibility_filter_can_be_disabled_for_future_targeting_rules() {
        assert!(score_candidate(
            1,
            EntityKind::Champion,
            1,
            true,
            true,
            false,
            100,
            100,
            10,
            0,
            TeamRelation::Any,
            false,
            100,
            100,
            1
        )
        .is_some());
    }

    #[test]
    fn friendly_and_any_relations_are_available_for_future_targeted_skills() {
        assert!(score_candidate(
            1,
            EntityKind::Champion,
            0,
            true,
            true,
            true,
            100,
            100,
            10,
            0,
            TeamRelation::Friendly,
            true,
            100,
            100,
            1
        )
        .is_some());
        assert!(score_candidate(
            2,
            EntityKind::Champion,
            1,
            true,
            true,
            true,
            100,
            100,
            10,
            0,
            TeamRelation::Any,
            true,
            100,
            100,
            1
        )
        .is_some());
    }

    #[test]
    fn screen_pixel_padding_expands_collision_geometry() {
        let collision_radius = 10;
        let sim_units_per_px = 2;
        let padding = pick_padding_px(EntityKind::Champion) * sim_units_per_px;
        let edge = 100 + collision_radius as u64 + padding;

        let candidate = score_candidate(
            1,
            EntityKind::Champion,
            1,
            true,
            true,
            true,
            100,
            100,
            collision_radius,
            0,
            TeamRelation::Hostile,
            true,
            edge,
            100,
            sim_units_per_px,
        )
        .expect("screen-pixel forgiveness should include the padded edge");
        assert_eq!(candidate.effective_radius, collision_radius as u64 + padding);

        assert!(score_candidate(
            1,
            EntityKind::Champion,
            1,
            true,
            true,
            true,
            100,
            100,
            collision_radius,
            0,
            TeamRelation::Hostile,
            true,
            edge + 1,
            100,
            sim_units_per_px,
        )
        .is_none());
    }

    #[test]
    fn zero_scale_preserves_exact_collision_geometry() {
        let candidate = score_candidate(
            1,
            EntityKind::Tower,
            1,
            true,
            true,
            true,
            100,
            100,
            10,
            0,
            TeamRelation::Hostile,
            true,
            110,
            100,
            0,
        )
        .expect("collision edge should remain selectable");
        assert_eq!(candidate.effective_radius, 10);

        assert!(score_candidate(
            1,
            EntityKind::Tower,
            1,
            true,
            true,
            true,
            100,
            100,
            10,
            0,
            TeamRelation::Hostile,
            true,
            111,
            100,
            0,
        )
        .is_none());
    }

    #[test]
    fn overlap_priority_is_champion_then_building_then_minion() {
        let champion = score(9, EntityKind::Champion, 900, 40);
        let tower = score(4, EntityKind::Tower, 100, 60);
        let other_objective = score(3, EntityKind::Other, 64, 60);
        let minion = score(1, EntityKind::Minion, 1, 20);

        assert!(score_is_better(champion, tower));
        assert!(score_is_better(champion, minion));
        assert!(score_is_better(tower, minion));
        assert!(score_is_better(other_objective, minion));
        assert!(!score_is_better(minion, tower));
    }

    #[test]
    fn nearest_center_wins_within_the_same_priority_tier() {
        let near = score(1, EntityKind::Champion, 9, 40);
        let far = score(2, EntityKind::Champion, 16, 40);

        assert!(score_is_better(near, far));
        assert!(!score_is_better(far, near));
    }

    #[test]
    fn smaller_hit_region_breaks_equal_distance_ties() {
        let precise = score(9, EntityKind::Tower, 25, 40);
        let broad = score(1, EntityKind::Tower, 25, 60);

        assert!(score_is_better(precise, broad));
    }
}
