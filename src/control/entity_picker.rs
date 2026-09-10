//! Generic click-to-entity resolution for direct control.
//!
//! The render thread publishes only a simulation-space cursor point. Entity identity is resolved
//! inside the paced simulation callback so the chosen target belongs to the exact simulation state
//! that will consume the command.

use mod_api_stable::StableSim;

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

fn score_candidate(
    id: usize,
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
    minimum_pick_radius: u64,
) -> Option<CandidateScore> {
    if !alive
        || !targetable
        || (require_visible && !visible)
        || !relation_matches(entity_team, controlled_team, relation)
    {
        return None;
    }

    let collision_radius = u64::try_from(collision_radius).unwrap_or(u64::MAX);
    let effective_radius = collision_radius.max(minimum_pick_radius);
    let dx = x.abs_diff(click_x) as u128;
    let dy = y.abs_diff(click_y) as u128;
    let distance_sq = dx * dx + dy * dy;
    let radius_sq = effective_radius as u128 * effective_radius as u128;

    (distance_sq <= radius_sq).then_some(CandidateScore {
        id,
        effective_radius,
        distance_sq,
    })
}

fn score_is_better(candidate: CandidateScore, current: CandidateScore) -> bool {
    candidate.distance_sq < current.distance_sq
        || (candidate.distance_sq == current.distance_sq
            && (candidate.effective_radius < current.effective_radius
                || (candidate.effective_radius == current.effective_radius
                    && candidate.id < current.id)))
}

pub fn pick_entity(
    sim: &StableSim<'_>,
    controlled_team: usize,
    relation: TeamRelation,
    require_visible: bool,
    click_x: u64,
    click_y: u64,
    minimum_pick_radius: u64,
) -> Option<EntityPick> {
    let mut best: Option<(CandidateScore, EntityPick)> = None;

    for index in 0..sim.entity_count() {
        let Some(entity) = sim.entity_at(index) else {
            continue;
        };

        let (x, y) = entity.pos();
        let collision_radius = entity.radius();
        let Some(score) = score_candidate(
            entity.id(),
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
            minimum_pick_radius,
        ) else {
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
    minimum_pick_radius: u64,
) -> Option<EntityPick> {
    pick_entity(
        sim,
        controlled_team,
        TeamRelation::Hostile,
        true,
        click_x,
        click_y,
        minimum_pick_radius,
    )
}

#[cfg(test)]
mod tests {
    use super::{score_candidate, score_is_better, CandidateScore, TeamRelation};

    #[test]
    fn relation_filter_rejects_wrong_team_dead_untargetable_and_hidden_entities() {
        assert!(score_candidate(
            1,
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
            10
        )
        .is_none());
        assert!(score_candidate(
            2,
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
            10
        )
        .is_none());
        assert!(score_candidate(
            3,
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
            10
        )
        .is_none());
        assert!(score_candidate(
            4,
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
            10
        )
        .is_none());
    }

    #[test]
    fn visibility_filter_can_be_disabled_for_future_targeting_rules() {
        assert!(score_candidate(
            1,
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
            10
        )
        .is_some());
    }

    #[test]
    fn friendly_and_any_relations_are_available_for_future_targeted_skills() {
        assert!(score_candidate(
            1,
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
            10
        )
        .is_some());
        assert!(score_candidate(
            2,
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
            10
        )
        .is_some());
    }

    #[test]
    fn minimum_radius_makes_small_entities_clickable() {
        let score = score_candidate(
            1,
            1,
            true,
            true,
            true,
            100,
            100,
            2,
            0,
            TeamRelation::Hostile,
            true,
            108,
            100,
            10,
        )
        .expect("inside minimum radius");
        assert_eq!(score.effective_radius, 10);

        assert!(score_candidate(
            1,
            1,
            true,
            true,
            true,
            100,
            100,
            2,
            0,
            TeamRelation::Hostile,
            true,
            111,
            100,
            10
        )
        .is_none());
    }

    #[test]
    fn nearest_center_wins_over_larger_overlapping_hit_region() {
        let near = CandidateScore {
            id: 1,
            effective_radius: 10,
            distance_sq: 9,
        };
        let far_but_large = CandidateScore {
            id: 2,
            effective_radius: 100,
            distance_sq: 16,
        };

        assert!(score_is_better(near, far_but_large));
        assert!(!score_is_better(far_but_large, near));
    }

    #[test]
    fn smaller_hit_region_breaks_equal_distance_ties() {
        let precise = CandidateScore {
            id: 9,
            effective_radius: 10,
            distance_sq: 25,
        };
        let broad = CandidateScore {
            id: 1,
            effective_radius: 30,
            distance_sq: 25,
        };

        assert!(score_is_better(precise, broad));
    }
}
