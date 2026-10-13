use figkit_model::{BlockMetrics, Bounds, ErrorKind, Point, Size};
use figkit_placement::*;
fn problem(profile: Profile) -> PlacementProblem {
    PlacementProblem {
        region: Bounds {
            x: 0.,
            y: 0.,
            width: 20.,
            height: 12.,
        },
        items: vec![PlacementItem {
            id: "item".into(),
            alternatives: vec![Alternative {
                id: "block".into(),
                metrics: BlockMetrics {
                    size: Size {
                        width: 10.,
                        height: 5.,
                    },
                    baseline: None,
                },
            }],
            allowed_regions: vec![],
            preferred_positions: vec![Point { x: 0., y: 0. }],
            target_groups: vec![],
        }],
        obstacles: vec![],
        segments: vec![],
        profile,
        em: 10.,
        connector_width: 1.,
        head_scale: 1.,
    }
}
#[test]
fn supplied_alternative_and_input_survive_deterministic_placement() {
    for profile in [Profile::LegacyFamily, Profile::LegacySpecial] {
        let p = problem(profile);
        let before = serde_json::to_value(&p).unwrap();
        let a = place(&p, &PlacementBudget::default()).unwrap();
        assert_eq!(a.placements.len(), 1);
        assert!(a.unresolved.is_empty());
        assert_eq!(a.placements[0].item.0, "item");
        assert_eq!(a.placements[0].alternative.0, "block");
        assert_eq!(a.placements[0].translation, Point { x: 0., y: 0. });
        assert!(a.placements[0].connectors.is_empty());
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            serde_json::to_value(place(&p, &PlacementBudget::default()).unwrap()).unwrap()
        );
        assert_eq!(serde_json::to_value(&p).unwrap(), before);
    }
    let mut empty = problem(Profile::LegacyFamily);
    empty.items.clear();
    let result = place(
        &empty,
        &PlacementBudget {
            max_occupancy_cells: 0,
            ..PlacementBudget::default()
        },
    )
    .unwrap();
    assert!(result.placements.is_empty());
    assert!(result.unresolved.is_empty());
    assert_eq!(result.work.occupancy_cells, 0);
}
#[test]
fn completed_obstruction_is_distinct_from_exhausted_search() {
    let mut p = problem(Profile::LegacyFamily);
    p.obstacles.push(Obstacle {
        bounds: p.region.clone(),
        clearance: 0.,
        treatment: ObstacleTreatment::Hard,
    });
    let complete = place(&p, &PlacementBudget::default()).unwrap();
    assert!(complete.placements.is_empty());
    assert_eq!(
        complete.unresolved[0].reason,
        UnresolvedReason::NoFeasibleCandidate
    );
    let short = place(
        &p,
        &PlacementBudget {
            max_candidates: 1,
            ..PlacementBudget::default()
        },
    )
    .unwrap();
    assert_eq!(short.unresolved[0].reason, UnresolvedReason::BudgetExceeded);
    let p = problem(Profile::LegacyFamily);
    assert_eq!(
        place(
            &p,
            &PlacementBudget {
                max_occupancy_cells: 0,
                ..PlacementBudget::default()
            }
        )
        .unwrap_err()
        .kind,
        ErrorKind::BudgetExceeded
    );
}
#[test]
fn hard_obstacles_eliminate_preferences_and_size_selects_supplied_block() {
    let mut p = problem(Profile::LegacySpecial);
    p.items[0].alternatives.insert(
        0,
        Alternative {
            id: "too-large".into(),
            metrics: BlockMetrics {
                size: Size {
                    width: 30.,
                    height: 30.,
                },
                baseline: None,
            },
        },
    );
    p.obstacles.push(Obstacle {
        bounds: Bounds {
            x: 0.,
            y: 0.,
            width: 10.,
            height: 5.,
        },
        clearance: 1.,
        treatment: ObstacleTreatment::Hard,
    });
    let out = place(&p, &PlacementBudget::default()).unwrap();
    assert_eq!(out.placements.len(), 1);
    assert_eq!(out.placements[0].alternative.0, "block");
    let xy = &out.placements[0].translation;
    assert!(xy.x >= 11. || xy.y >= 6., "hard clearance violated: {xy:?}");
}
#[test]
fn occupancy_allocation_stamp_and_clone_limits_are_inclusive() {
    // 2x2 physical raster has (ceil(2)+2)^2 = 16 cells.
    for cap in [15, 16, 17] {
        let budget = Budget::new(&PlacementBudget {
            max_occupancy_cells: cap,
            ..PlacementBudget::default()
        });
        assert_eq!(
            Occupancy::new(2., 2., Profile::LegacyFamily, &budget).is_ok(),
            cap >= 16
        );
    }
    for cap in [31, 32, 33] {
        let budget = Budget::new(&PlacementBudget {
            max_occupancy_cells: cap,
            ..PlacementBudget::default()
        });
        let occ = Occupancy::new(2., 2., Profile::LegacyFamily, &budget).unwrap();
        assert_eq!(occ.try_clone().is_ok(), cap >= 32);
    }
    for cap in [3, 4, 5] {
        let budget = Budget::new(&PlacementBudget {
            max_raster_work: cap,
            ..PlacementBudget::default()
        });
        let mut occ = Occupancy::new(2., 2., Profile::LegacyFamily, &budget).unwrap();
        assert_eq!(occ.rect([0., 0., 2., 2.]).is_ok(), cap >= 4);
    }
    let budget = Budget::new(&PlacementBudget::default());
    assert_eq!(
        Occupancy::new(f64::MAX, f64::MAX, Profile::LegacyFamily, &budget)
            .unwrap_err()
            .kind,
        ErrorKind::BudgetExceeded
    );
    assert_eq!(budget.work().occupancy_cells, 0);
}
#[test]
fn finite_grid_candidate_and_scoring_counters_include_every_probe() {
    // 2x2 labels in a 4x4 region, grid step2: (0,0),(2,0),(0,2),(2,2).
    for cap in [3, 4, 5] {
        let budget = Budget::new(&PlacementBudget {
            max_candidates: cap,
            ..PlacementBudget::default()
        });
        let mut occ = Occupancy::new(4., 4., Profile::LegacyFamily, &budget).unwrap();
        let result = candidates(
            &[(2., 2.)],
            [0., 0., 4., 4.],
            [0., 0., 4., 4.],
            &[],
            10.,
            &mut occ,
            &budget,
        );
        assert_eq!(result.is_ok(), cap >= 4);
        if let Ok(values) = result {
            assert_eq!(values.len(), 4);
            assert_eq!(budget.work().candidates, 4);
        }
    }
    for cap in [3, 4, 5] {
        let budget = Budget::new(&PlacementBudget {
            max_scoring_work: cap,
            ..PlacementBudget::default()
        });
        let mut occ = Occupancy::new(4., 4., Profile::LegacyFamily, &budget).unwrap();
        assert_eq!(
            candidates(
                &[(2., 2.)],
                [0., 0., 4., 4.],
                [0., 0., 4., 4.],
                &[],
                10.,
                &mut occ,
                &budget
            )
            .is_ok(),
            cap >= 4
        );
    }
}
#[test]
fn route_trials_and_connector_samples_are_bounded_separately() {
    let options = RouteOptions {
        origin: (0., 0.),
        profile: Profile::LegacyFamily,
        width: 100.,
        height: 80.,
        em: 10.,
        head_length: 2.,
    };
    for cap in [0, 1, 2] {
        let budget = Budget::new(&PlacementBudget {
            max_routes: cap,
            ..PlacementBudget::default()
        });
        let hard = Occupancy::new(100., 80., Profile::LegacyFamily, &budget).unwrap();
        let soft = Occupancy::new(100., 80., Profile::LegacyFamily, &budget).unwrap();
        let out = route(
            &options,
            &[Start {
                position: (10., 10.),
                normal: (1., 0.),
                cost: 0.,
            }],
            &[Target {
                position: (50., 10.),
                clearance: 0.,
                cost: 0.,
            }],
            None,
            &hard,
            &soft,
            &budget,
        );
        assert_eq!(out.is_ok(), cap >= 1);
        if let Ok(route) = out {
            assert!(route.is_some());
        }
    }
    let budget = Budget::new(&PlacementBudget {
        max_route_samples: 0,
        ..PlacementBudget::default()
    });
    let hard = Occupancy::new(100., 80., Profile::LegacyFamily, &budget).unwrap();
    let soft = Occupancy::new(100., 80., Profile::LegacyFamily, &budget).unwrap();
    assert_eq!(
        route(
            &options,
            &[Start {
                position: (10., 10.),
                normal: (1., 0.),
                cost: 0.
            }],
            &[Target {
                position: (50., 10.),
                clearance: 0.,
                cost: 0.
            }],
            None,
            &hard,
            &soft,
            &budget
        )
        .unwrap_err()
        .kind,
        ErrorKind::BudgetExceeded
    );
}

#[test]
fn family_connector_preserves_pre_extraction_fractional_head_rounding() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/family_head.json")).unwrap();
    let budget = Budget::new(&PlacementBudget::default());
    let route = Route {
        score: 0.,
        start: (0., 0.),
        control: (20., 0.),
        end: (40., 0.),
    };
    let connector =
        routing::connector_with_profile(&route, 0.1025, 1., Profile::LegacyFamily, &budget)
            .unwrap()
            .unwrap();
    let expected = fixture["head"].as_array().unwrap();
    assert_eq!(connector.head.len(), expected.len());
    for (i, (actual, expected)) in connector.head.iter().zip(expected).enumerate() {
        assert!(
            (actual.x - expected[0].as_f64().unwrap()).abs() < 1e-12,
            "head[{i}] x: {actual:?}"
        );
        assert!(
            (actual.y - expected[1].as_f64().unwrap()).abs() < 1e-12,
            "head[{i}] y: {actual:?}"
        );
    }
    // The alternate legacy profile differs on this genuine baseline rounding tie.
    let other =
        routing::connector_with_profile(&route, 0.1025, 1., Profile::LegacySpecial, &budget)
            .unwrap()
            .unwrap();
    assert_ne!(connector.head, other.head);
}
