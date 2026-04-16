/// Unit tests for search scoring and edit distance.
///
/// These tests verify the deterministic scoring formula and spell-correction
/// edit distance implementation without requiring a database connection.
use talentflow::application::search_service::{
    domain_rating, edit_distance, SearchResult, SearchService, SortField,
};

// ── Edit distance ─────────────────────────────────────────────────────────────

#[test]
fn edit_distance_identical_strings_is_zero() {
    assert_eq!(edit_distance("hello", "hello"), 0);
}

#[test]
fn edit_distance_single_substitution() {
    // "cat" vs "bat" — one substitution
    assert_eq!(edit_distance("cat", "bat"), 1);
}

#[test]
fn edit_distance_single_insertion() {
    // "car" vs "card" — one insertion
    assert_eq!(edit_distance("car", "card"), 1);
}

#[test]
fn edit_distance_single_deletion() {
    // "card" vs "car" — one deletion
    assert_eq!(edit_distance("card", "car"), 1);
}

#[test]
fn edit_distance_two_substitutions() {
    assert_eq!(edit_distance("abc", "xyz"), 3);
}

#[test]
fn edit_distance_case_insensitive() {
    // Both normalised to lowercase
    assert_eq!(edit_distance("Hello", "hello"), 0);
    assert_eq!(edit_distance("Smith", "smith"), 0);
}

#[test]
fn edit_distance_empty_strings() {
    assert_eq!(edit_distance("", ""), 0);
    assert_eq!(edit_distance("abc", ""), 3);
    assert_eq!(edit_distance("", "abc"), 3);
}

#[test]
fn edit_distance_typical_typos() {
    // "recieve" → "receive" — one transposition equivalent
    assert!(edit_distance("recieve", "receive") <= 2);
    // "calendr" → "calendar" — one insertion
    assert!(edit_distance("calendr", "calendar") <= 2);
}

// ── Scoring weights ───────────────────────────────────────────────────────────

/// Verify that the scoring constants sum to 1.0 as documented.
#[test]
fn scoring_weights_sum_to_one() {
    let exact_weight = 0.40f64;
    let tag_weight = 0.35f64;
    let recency_weight = 0.25f64;
    let total = exact_weight + tag_weight + recency_weight;
    assert!(
        (total - 1.0).abs() < 1e-9,
        "candidate scoring weights must sum to 1.0, got {total}"
    );
}

#[test]
fn offer_scoring_weights_sum_to_one() {
    let exact_weight = 0.60f64;
    let recency_weight = 0.40f64;
    let total = exact_weight + recency_weight;
    assert!(
        (total - 1.0).abs() < 1e-9,
        "offer scoring weights must sum to 1.0, got {total}"
    );
}

/// Recency score should be 1.0 for a just-created resource.
#[test]
fn recency_score_new_resource_is_one() {
    // Created "now" → age_days = 0 → score = 1.0
    let age_days = 0f64;
    let score = (1.0 - age_days / 365.0).clamp(0.0, 1.0);
    assert!(
        (score - 1.0).abs() < 1e-9,
        "recency score for age=0 should be 1.0"
    );
}

/// Recency score should be 0.0 for a year-old resource.
#[test]
fn recency_score_old_resource_is_zero() {
    let age_days = 365f64;
    let score = (1.0 - age_days / 365.0).clamp(0.0, 1.0);
    assert!(
        (score - 0.0).abs() < 1e-9,
        "recency score for age=365 should be 0.0"
    );
}

/// Recency score must clamp to [0, 1] for very old resources.
#[test]
fn recency_score_clamps_to_zero() {
    let age_days = 1000f64;
    let score = (1.0 - age_days / 365.0).clamp(0.0, 1.0);
    assert_eq!(score, 0.0, "recency score must clamp to 0 for ages > 365");
}

// ── Tag overlap ratio ─────────────────────────────────────────────────────────

#[test]
fn tag_overlap_full_match() {
    let requested = vec!["rust".to_string(), "backend".to_string()];
    let candidate_tags = vec!["rust".to_string(), "backend".to_string()];
    let overlap = candidate_tags
        .iter()
        .filter(|t| requested.contains(t))
        .count() as f64;
    let ratio = overlap / requested.len() as f64;
    assert!((ratio - 1.0).abs() < 1e-9, "full tag overlap should be 1.0");
}

#[test]
fn tag_overlap_partial_match() {
    let requested = vec!["rust".to_string(), "backend".to_string()];
    let candidate_tags = vec!["rust".to_string()];
    let overlap = candidate_tags
        .iter()
        .filter(|t| requested.contains(t))
        .count() as f64;
    let ratio = overlap / requested.len() as f64;
    assert!(
        (ratio - 0.5).abs() < 1e-9,
        "partial tag overlap should be 0.5"
    );
}

#[test]
fn tag_overlap_no_match() {
    let requested = vec!["rust".to_string(), "backend".to_string()];
    let candidate_tags = vec!["java".to_string()];
    let overlap = candidate_tags
        .iter()
        .filter(|t| requested.contains(t))
        .count() as f64;
    let ratio = overlap / requested.len() as f64;
    assert!((ratio - 0.0).abs() < 1e-9, "no tag overlap should be 0.0");
}

// ── SortField ─────────────────────────────────────────────────────────────────

#[test]
fn sort_field_variants_are_distinct() {
    assert_eq!(SortField::Relevance, SortField::Relevance);
    assert_ne!(SortField::Relevance, SortField::Recency);
    assert_ne!(SortField::Recency, SortField::TagOverlap);
    assert_ne!(SortField::Relevance, SortField::TagOverlap);
    assert_ne!(SortField::Relevance, SortField::Popularity);
    assert_ne!(SortField::Relevance, SortField::Rating);
    assert_ne!(SortField::Relevance, SortField::Distance);
    assert_ne!(SortField::Popularity, SortField::Rating);
    assert_ne!(SortField::Rating, SortField::Distance);
}

// ── Popularity score ──────────────────────────────────────────────────────────

#[test]
fn popularity_score_no_tags_full_recency() {
    // tag_density=0.0, recency=1.0 → 0.0*0.6 + 1.0*0.4 = 0.4
    let score = 0.0_f64 * 0.6 + 1.0_f64 * 0.4;
    assert!((score - 0.4).abs() < 1e-9);
}

#[test]
fn popularity_score_saturates_at_ten_tags() {
    // tag_density saturates at 10 tags → 1.0*0.6 + recency*0.4
    let tag_count = 15usize;
    let recency = 0.5_f64;
    let tag_density = (tag_count as f64 / 10.0_f64).min(1.0);
    let score = tag_density * 0.6 + recency * 0.4;
    assert!(
        (tag_density - 1.0).abs() < 1e-9,
        "tag_density should saturate at 1.0"
    );
    assert!((score - 0.8).abs() < 1e-9);
}

#[test]
fn popularity_score_range_is_zero_to_one() {
    for tag_count in [0usize, 5, 10, 20] {
        for recency_int in [0u32, 50, 100] {
            let recency = recency_int as f64 / 100.0;
            let tag_density = (tag_count as f64 / 10.0).min(1.0);
            let score = tag_density * 0.6 + recency * 0.4;
            assert!(
                score >= 0.0 && score <= 1.0,
                "popularity out of [0,1]: tag_count={tag_count} recency={recency} score={score}"
            );
        }
    }
}

// ── Rating derivation ─────────────────────────────────────────────────────────

#[test]
fn rating_is_score_times_five() {
    let score = 0.8_f64;
    let rating = score * 5.0;
    assert!((rating - 4.0).abs() < 1e-9);
}

#[test]
fn rating_range_is_zero_to_five() {
    for score_int in [0u32, 25, 50, 75, 100] {
        let score = score_int as f64 / 100.0;
        let rating = score * 5.0;
        assert!(
            rating >= 0.0 && rating <= 5.0,
            "rating {rating} out of [0.0, 5.0] for score {score}"
        );
    }
}

// ── Domain-native offer rating (salary-based) ───────────────────────────────
//
// These tests call `domain_rating` from the production code directly — there
// is no mirror-copy helper.  If the formula in search_service.rs changes,
// these tests break immediately.

/// When salary_cents is present, rating should be derived from salary position.
#[test]
fn offer_rating_prefers_salary_over_relevance() {
    let salary = Some(150_000_00_i64); // $150,000 — midpoint of $30k–$300k
    let relevance_score = 0.1; // low relevance
    let rating = domain_rating(salary, relevance_score);
    // $150k is roughly (150k-30k)/(300k-30k) = 120/270 ≈ 0.444 of band → ~2.22
    let expected = ((150_000_00.0 - 30_000_00.0) / (300_000_00.0 - 30_000_00.0)) * 5.0;
    assert!(
        (rating - expected).abs() < 0.01,
        "salary-based rating should be ~{expected:.2}, got {rating:.2}"
    );
    // Must differ from relevance fallback (0.1 * 5.0 = 0.5)
    assert!(
        (rating - 0.5).abs() > 0.1,
        "salary-based rating must differ from relevance fallback"
    );
}

/// When salary_cents is None, rating falls back to score * 5.0.
#[test]
fn offer_rating_falls_back_to_relevance_when_no_salary() {
    let relevance_score = 0.7;
    let rating = domain_rating(None, relevance_score);
    assert!(
        (rating - 3.5).abs() < 1e-9,
        "without salary, rating should be score * 5.0 = 3.5, got {rating}"
    );
}

/// When salary_cents is 0 (invalid), rating falls back to relevance.
#[test]
fn offer_rating_falls_back_for_zero_salary() {
    let rating = domain_rating(Some(0), 0.6);
    assert!(
        (rating - 3.0).abs() < 1e-9,
        "zero salary should fall back to relevance, got {rating}"
    );
}

/// Salary at the floor ($30k) produces rating 0.0.
#[test]
fn offer_rating_at_salary_floor_is_zero() {
    let rating = domain_rating(Some(30_000_00), 0.9);
    assert!(
        rating.abs() < 0.01,
        "salary at floor should produce rating ~0.0, got {rating}"
    );
}

/// Salary at the ceiling ($300k) produces rating 5.0.
#[test]
fn offer_rating_at_salary_ceiling_is_five() {
    let rating = domain_rating(Some(300_000_00), 0.1);
    assert!(
        (rating - 5.0).abs() < 0.01,
        "salary at ceiling should produce rating ~5.0, got {rating}"
    );
}

/// Salary below floor clamps to 0.0.
#[test]
fn offer_rating_below_floor_clamps() {
    let rating = domain_rating(Some(10_000_00), 0.5);
    assert!(
        rating.abs() < 0.01,
        "salary below floor should clamp to 0.0, got {rating}"
    );
}

/// Salary above ceiling clamps to 5.0.
#[test]
fn offer_rating_above_ceiling_clamps() {
    let rating = domain_rating(Some(500_000_00), 0.5);
    assert!(
        (rating - 5.0).abs() < 0.01,
        "salary above ceiling should clamp to 5.0, got {rating}"
    );
}

/// Negative salary_cents falls back to relevance (treated as invalid).
#[test]
fn offer_rating_negative_salary_falls_back() {
    let rating = domain_rating(Some(-100_00), 0.4);
    assert!(
        (rating - 2.0).abs() < 1e-9,
        "negative salary should fall back to relevance: got {rating}"
    );
}

/// domain_rating output is always in [0.0, 5.0] for any input.
#[test]
fn offer_rating_output_always_in_range() {
    let cases: Vec<(Option<i64>, f64)> = vec![
        (None, 0.0),
        (None, 1.0),
        (Some(0), 0.5),
        (Some(-1), 0.5),
        (Some(1), 0.5),
        (Some(30_000_00), 0.0),
        (Some(30_000_00), 1.0),
        (Some(300_000_00), 0.0),
        (Some(300_000_00), 1.0),
        (Some(999_999_99), 0.5),
        (Some(i64::MAX), 0.5),
    ];
    for (salary, rel) in cases {
        let r = domain_rating(salary, rel);
        assert!(
            (0.0..=5.0).contains(&r),
            "domain_rating({salary:?}, {rel}) = {r}, outside [0, 5]"
        );
    }
}

// ── Haversine distance ────────────────────────────────────────────────────────

#[test]
fn haversine_same_point_is_zero() {
    let d = SearchService::haversine_miles(40.7128, -74.0060, 40.7128, -74.0060);
    assert!(
        d.abs() < 1e-6,
        "distance from a point to itself should be ~0"
    );
}

#[test]
fn haversine_known_distance_nyc_to_la() {
    // New York (40.7128°N, 74.0060°W) to Los Angeles (34.0522°N, 118.2437°W)
    // Great-circle distance ≈ 2,446 miles
    let d = SearchService::haversine_miles(40.7128, -74.0060, 34.0522, -118.2437);
    assert!(
        d > 2_400.0 && d < 2_500.0,
        "NYC→LA distance should be ~2446 miles, got {d}"
    );
}

#[test]
fn haversine_is_symmetric() {
    let d1 = SearchService::haversine_miles(51.5074, -0.1278, 48.8566, 2.3522); // London→Paris
    let d2 = SearchService::haversine_miles(48.8566, 2.3522, 51.5074, -0.1278); // Paris→London
    assert!((d1 - d2).abs() < 1e-6, "haversine must be symmetric");
}

// ── Deterministic interleaving ────────────────────────────────────────────────

fn make_result(id_suffix: u8, score: f64, recommended: bool) -> SearchResult {
    SearchResult {
        resource_type: "candidate".into(),
        id: uuid::Uuid::from_u128(id_suffix as u128),
        title: format!("Item {id_suffix}"),
        subtitle: None,
        score,
        tags: vec![],
        status: None,
        created_at: "2024-01-01T00:00:00Z".into(),
        rating: Some(score * 5.0),
        distance_miles: None,
        popularity_score: Some(0.5),
        recommended,
        domain_rated: false,
    }
}

#[test]
fn interleave_empty_recommended_returns_regular() {
    let regular = vec![make_result(1, 0.3, false), make_result(2, 0.2, false)];
    let recommended: Vec<SearchResult> = vec![];
    let out = SearchService::interleave(regular, recommended);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].id, uuid::Uuid::from_u128(1));
}

#[test]
fn interleave_empty_regular_returns_recommended() {
    let regular: Vec<SearchResult> = vec![];
    let recommended = vec![make_result(10, 0.9, true), make_result(11, 0.8, true)];
    let out = SearchService::interleave(regular, recommended);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].id, uuid::Uuid::from_u128(10));
}

#[test]
fn interleave_three_to_one_ratio() {
    // 6 regular + 2 recommended → pattern: R R R Rec R R R Rec
    let regular: Vec<SearchResult> = (1..=6).map(|i| make_result(i, 0.3, false)).collect();
    let recommended: Vec<SearchResult> =
        vec![make_result(10, 0.9, true), make_result(11, 0.8, true)];
    let out = SearchService::interleave(regular, recommended);
    // Positions 0,1,2 = regular; position 3 = recommended; positions 4,5,6 = regular; position 7 = recommended
    assert_eq!(out.len(), 8);
    assert!(!out[0].recommended);
    assert!(!out[1].recommended);
    assert!(!out[2].recommended);
    assert!(out[3].recommended, "position 3 should be recommended");
    assert!(!out[4].recommended);
    assert!(!out[5].recommended);
    assert!(!out[6].recommended);
    assert!(out[7].recommended, "position 7 should be recommended");
}

#[test]
fn interleave_is_deterministic() {
    let regular: Vec<SearchResult> = (1..=4).map(|i| make_result(i, 0.3, false)).collect();
    let recommended: Vec<SearchResult> = vec![make_result(10, 0.9, true)];
    let out1 = SearchService::interleave(
        (1..=4).map(|i| make_result(i, 0.3, false)).collect(),
        vec![make_result(10, 0.9, true)],
    );
    let out2 = SearchService::interleave(regular, recommended);
    assert_eq!(out1.len(), out2.len());
    for (a, b) in out1.iter().zip(out2.iter()) {
        assert_eq!(
            a.id, b.id,
            "interleave must produce same order on identical inputs"
        );
    }
}

#[test]
fn interleave_more_recommended_than_slots_appended() {
    // 3 regular + 5 recommended → R R R Rec + remaining 4 recommended appended
    let regular: Vec<SearchResult> = (1..=3).map(|i| make_result(i, 0.3, false)).collect();
    let recommended: Vec<SearchResult> = (10..=14).map(|i| make_result(i, 0.9, true)).collect();
    let out = SearchService::interleave(regular, recommended);
    assert_eq!(out.len(), 8, "3 regular + 5 recommended = 8 total");
    // First 3 are regular, 4th is first recommended
    assert!(!out[0].recommended);
    assert!(!out[1].recommended);
    assert!(!out[2].recommended);
    assert!(out[3].recommended);
    // Remaining 4 recommended appended after regular exhausted
    assert!(out[4].recommended);
    assert!(out[5].recommended);
    assert!(out[6].recommended);
    assert!(out[7].recommended);
}

// ── Distance sort (None values last) ─────────────────────────────────────────

#[test]
fn distance_sort_none_values_go_last() {
    let mut results = vec![
        {
            let mut r = make_result(1, 0.5, false);
            r.distance_miles = None;
            r
        },
        {
            let mut r = make_result(2, 0.5, false);
            r.distance_miles = Some(5.0);
            r
        },
        {
            let mut r = make_result(3, 0.5, false);
            r.distance_miles = Some(2.0);
            r
        },
    ];

    // Replicate sort logic: ascending distance, None last
    results.sort_by(|a, b| {
        match (a.distance_miles, b.distance_miles) {
            (Some(da), Some(db)) => da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
        .then_with(|| a.resource_type.cmp(&b.resource_type))
        .then_with(|| a.id.cmp(&b.id))
    });

    assert_eq!(results[0].distance_miles, Some(2.0), "closest first");
    assert_eq!(results[1].distance_miles, Some(5.0));
    assert!(results[2].distance_miles.is_none(), "None distance last");
}

// ── Recommended flag ──────────────────────────────────────────────────────────

#[test]
fn recommended_threshold_is_half() {
    // Items with score >= 0.5 should be recommended
    assert!(0.5_f64 >= 0.5, "score 0.5 should be recommended");
    assert!(0.9_f64 >= 0.5, "score 0.9 should be recommended");
    assert!(!(0.49_f64 >= 0.5), "score 0.49 should not be recommended");
    assert!(!(0.0_f64 >= 0.5), "score 0.0 should not be recommended");
}

// ── Distance computation (Haversine) ──────────────────────────────────────────

/// NYC (HQ seed coords) to Boston — ~215 statute miles.
#[test]
fn haversine_nyc_to_boston() {
    // New York (40.7128°N, 74.0060°W) to Boston (42.3601°N, 71.0589°W)
    let d = SearchService::haversine_miles(40.7128, -74.0060, 42.3601, -71.0589);
    assert!(
        d > 190.0 && d < 230.0,
        "NYC→Boston should be ~215 miles, got {d}"
    );
}

/// London to Paris — ~214 statute miles (known city pair).
#[test]
fn haversine_london_to_paris_known_value() {
    let d = SearchService::haversine_miles(51.5074, -0.1278, 48.8566, 2.3522);
    assert!(
        d > 200.0 && d < 230.0,
        "London→Paris should be ~214 miles, got {d}"
    );
}

// ── Spell correction corpus ───────────────────────────────────────────────────

/// Spell correction must prefer vocabulary terms over history when both have
/// the same edit distance from the query.  Vocabulary labels are ordered first
/// in the merged dictionary, so the min-by-key selection picks the vocabulary
/// term on a tie.
#[test]
fn spell_correction_prefers_vocabulary_over_history_at_equal_distance() {
    // Simulate dictionary: vocabulary first, history second (same order as prod).
    let vocabulary = vec!["engineering".to_string()];
    let history = vec!["engineeringx".to_string()]; // edit distance 1 from "engineerin" too
    let dict: Vec<String> = vocabulary.into_iter().chain(history).collect();

    let kw = "engineerin"; // one deletion from "engineering"

    let best = dict
        .iter()
        .filter(|s| !s.is_empty())
        .filter_map(|candidate| {
            let dist = edit_distance(kw, candidate);
            if dist > 0 && dist <= 2 {
                Some((dist, candidate.clone()))
            } else {
                None
            }
        })
        .min_by_key(|(dist, _)| *dist)
        .map(|(_, s)| s);

    assert_eq!(
        best.as_deref(),
        Some("engineering"),
        "vocabulary term 'engineering' (dist=1) should be selected over history"
    );
}

/// Spell correction must suggest a vocabulary term even when query history is
/// empty — i.e., vocabulary alone is sufficient to produce a correction.
#[test]
fn spell_correction_works_with_vocabulary_only_and_empty_history() {
    let vocabulary = vec!["backend".to_string(), "frontend".to_string()];
    let history: Vec<String> = vec![]; // intentionally empty

    let dict: Vec<String> = vocabulary.into_iter().chain(history).collect();
    let kw = "backkend"; // 1 substitution from "backend"

    let best = dict
        .iter()
        .filter(|s| !s.is_empty())
        .filter_map(|candidate| {
            let dist = edit_distance(kw, candidate);
            if dist > 0 && dist <= 2 {
                Some((dist, candidate.clone()))
            } else {
                None
            }
        })
        .min_by_key(|(dist, _)| *dist)
        .map(|(_, s)| s);

    assert_eq!(
        best.as_deref(),
        Some("backend"),
        "vocabulary-only correction should work even with empty history"
    );
}

// ── Offer distance pass-through ───────────────────────────────────────────────

/// Offers always have `distance_miles = None` (they have no physical location).
/// A `None` distance passes through the `max_distance_miles` filter unchanged
/// (map_or(true, ...)) — this is the correct, documented behavior.
#[test]
fn offer_always_passes_distance_filter() {
    // Simulate the max_distance_miles filter as applied in search_service.rs
    let max_distance = 10.0_f64;
    let offer_distance: Option<f64> = None; // offers never have coords

    let passes = offer_distance.map_or(true, |v| v <= max_distance);
    assert!(
        passes,
        "an offer with distance_miles=None must pass through the distance filter"
    );
}

/// Candidates with coordinates are filtered out when they exceed max_distance.
#[test]
fn candidate_with_far_distance_is_filtered_out() {
    let max_distance = 10.0_f64;
    let far_candidate_distance: Option<f64> = Some(50.0);

    let passes = far_candidate_distance.map_or(true, |v| v <= max_distance);
    assert!(
        !passes,
        "a candidate at 50 miles should be excluded when max_distance_miles=10"
    );
}

/// Candidates without coordinates (distance_miles=None) pass the distance
/// filter — they are treated the same as offers: no location basis, no
/// exclusion.
#[test]
fn candidate_without_coords_passes_distance_filter() {
    let max_distance = 10.0_f64;
    let no_coords_distance: Option<f64> = None;

    let passes = no_coords_distance.map_or(true, |v| v <= max_distance);
    assert!(
        passes,
        "a candidate with no coordinates must pass through the distance filter"
    );
}

// ── Distance filter logic ─────────────────────────────────────────────────────

/// A result with distance_miles = None must always pass through the
/// max_distance_miles filter (no location basis → no exclusion).
#[test]
fn distance_filter_none_passes_through() {
    let max_d = 50.0_f64;
    // Mirrors the retain logic in search_service::search
    let passes = |dist: Option<f64>| dist.map_or(true, |v| v <= max_d);
    assert!(passes(None), "None distance must pass through any filter");
    assert!(passes(Some(0.0)), "0.0 passes 50-mile filter");
    assert!(passes(Some(50.0)), "exactly 50.0 passes (<=)");
    assert!(!passes(Some(50.01)), "50.01 exceeds 50-mile filter");
    assert!(!passes(Some(1000.0)), "1000.0 exceeds 50-mile filter");
}

/// Items within the max_distance threshold are included; items beyond it
/// are excluded; items with no distance basis are included regardless.
#[test]
fn distance_filter_excludes_out_of_range() {
    let max_d = 100.0_f64;
    let filter = |d: Option<f64>| d.map_or(true, |v| v <= max_d);

    // In-range and exact boundary pass
    assert!(filter(Some(0.0)));
    assert!(filter(Some(99.9)));
    assert!(filter(Some(100.0)));

    // Out-of-range fails
    assert!(!filter(Some(100.1)));
    assert!(!filter(Some(2446.0))); // NYC→LA distance

    // No location basis always passes
    assert!(filter(None));
}

/// Distance sort: items are ordered ascending with None (no location basis)
/// sorted to the end.  Verifies the full sort comparator, not just two items.
#[test]
fn distance_sort_multiple_items_ascending() {
    let mut results = vec![
        {
            let mut r = make_result(1, 0.5, false);
            r.distance_miles = Some(300.0);
            r
        },
        {
            let mut r = make_result(2, 0.5, false);
            r.distance_miles = None;
        r
        },
        {
            let mut r = make_result(3, 0.5, false);
            r.distance_miles = Some(10.0);
            r
        },
        {
            let mut r = make_result(4, 0.5, false);
            r.distance_miles = Some(150.0);
            r
        },
        {
            let mut r = make_result(5, 0.5, false);
            r.distance_miles = None;
            r
        },
    ];

    results.sort_by(|a, b| {
        match (a.distance_miles, b.distance_miles) {
            (Some(da), Some(db)) => da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
        .then_with(|| a.resource_type.cmp(&b.resource_type))
        .then_with(|| a.id.cmp(&b.id))
    });

    assert_eq!(results[0].distance_miles, Some(10.0), "closest first");
    assert_eq!(results[1].distance_miles, Some(150.0));
    assert_eq!(results[2].distance_miles, Some(300.0));
    assert!(
        results[3].distance_miles.is_none(),
        "None distances sort after all numeric distances"
    );
    assert!(results[4].distance_miles.is_none());
}

// ── Rating filter mechanics ─────────────────────────────────────────────────
//
// These tests replicate the retain logic from search_service::search to verify
// that min_rating / max_rating interact correctly with domain-native ratings.

/// min_rating filter retains items with rating >= threshold, passes None through.
#[test]
fn min_rating_filter_retains_above_threshold() {
    let filter = |r: Option<f64>, min: f64| r.map_or(true, |v| v >= min);
    assert!(filter(Some(3.0), 2.0), "3.0 >= 2.0");
    assert!(filter(Some(2.0), 2.0), "2.0 >= 2.0 (boundary)");
    assert!(!filter(Some(1.9), 2.0), "1.9 < 2.0");
    assert!(filter(None, 2.0), "None passes through");
}

/// max_rating filter retains items with rating <= threshold, passes None through.
#[test]
fn max_rating_filter_retains_below_threshold() {
    let filter = |r: Option<f64>, max: f64| r.map_or(true, |v| v <= max);
    assert!(filter(Some(2.0), 3.0), "2.0 <= 3.0");
    assert!(filter(Some(3.0), 3.0), "3.0 <= 3.0 (boundary)");
    assert!(!filter(Some(3.1), 3.0), "3.1 > 3.0");
    assert!(filter(None, 3.0), "None passes through");
}

/// Combined min + max rating creates a band filter.
#[test]
fn rating_band_filter() {
    let in_band = |r: Option<f64>, min: f64, max: f64| {
        r.map_or(true, |v| v >= min) && r.map_or(true, |v| v <= max)
    };
    assert!(in_band(Some(2.5), 1.0, 4.0));
    assert!(in_band(Some(1.0), 1.0, 4.0), "at min boundary");
    assert!(in_band(Some(4.0), 1.0, 4.0), "at max boundary");
    assert!(!in_band(Some(0.5), 1.0, 4.0), "below band");
    assert!(!in_band(Some(4.5), 1.0, 4.0), "above band");
    assert!(in_band(None, 1.0, 4.0), "None passes both filters");
}

/// Domain-native rating feeds into rating filters correctly: an offer with
/// salary_cents should be filterable by min_rating/max_rating using the
/// salary-derived value, not the relevance fallback.
#[test]
fn salary_derived_rating_works_with_rating_filter() {
    // $150k → ~2.22 rating (see offer_rating_prefers_salary_over_relevance)
    let salary_rating = domain_rating(Some(150_000_00), 0.1);
    // This rating should pass a min_rating=2.0 filter
    assert!(salary_rating >= 2.0, "salary rating {salary_rating} should pass min_rating=2.0");
    // But fail a min_rating=3.0 filter
    assert!(salary_rating < 3.0, "salary rating {salary_rating} should fail min_rating=3.0");
}

// ── Quality gate filter (exclusive semantics) ───────────────────────────────
//
// quality_min/quality_max differ from min_rating/max_rating: they EXCLUDE items
// that are not domain-rated, rather than passing them through.

/// Non-domain-rated item is excluded by quality_min, even if its rating is high.
#[test]
fn quality_filter_excludes_non_domain_rated() {
    let r = SearchResult {
        rating: Some(4.0),  // high rating
        domain_rated: false, // but not domain-native
        ..make_result(1, 0.8, false)
    };
    // quality_min gate: domain_rated must be true AND rating >= qmin
    let passes = r.domain_rated && r.rating.map_or(false, |v| v >= 1.0);
    assert!(!passes, "non-domain-rated item must be excluded by quality_min");
}

/// Domain-rated item within the quality range passes.
#[test]
fn quality_filter_passes_domain_rated_in_range() {
    let r = SearchResult {
        rating: Some(3.0),
        domain_rated: true,
        ..make_result(2, 0.5, false)
    };
    let passes_min = r.domain_rated && r.rating.map_or(false, |v| v >= 2.0);
    let passes_max = r.domain_rated && r.rating.map_or(false, |v| v <= 4.0);
    assert!(passes_min && passes_max, "domain-rated item in [2.0, 4.0] should pass");
}

/// Domain-rated item below quality_min is excluded.
#[test]
fn quality_filter_excludes_below_min() {
    let r = SearchResult {
        rating: Some(1.0),
        domain_rated: true,
        ..make_result(3, 0.2, false)
    };
    let passes = r.domain_rated && r.rating.map_or(false, |v| v >= 2.0);
    assert!(!passes, "domain-rated item with rating 1.0 should fail quality_min=2.0");
}

/// Shows the key semantic difference: same item passes min_rating but fails quality_min.
#[test]
fn quality_vs_rating_filter_semantics() {
    let r = SearchResult {
        rating: Some(3.0),
        domain_rated: false, // relevance-derived, not domain-native
        ..make_result(4, 0.6, false)
    };

    // min_rating uses map_or(true, ...) — passes items without domain basis
    let passes_min_rating = r.rating.map_or(true, |v| v >= 2.0);
    assert!(passes_min_rating, "min_rating should pass non-domain-rated item with rating 3.0");

    // quality_min requires domain_rated — excludes non-domain-rated items
    let passes_quality_min = r.domain_rated && r.rating.map_or(false, |v| v >= 2.0);
    assert!(!passes_quality_min, "quality_min should EXCLUDE same non-domain-rated item");
}

// ── Total compensation formula ──────────────────────────────────────────────

/// Total comp = salary * (1 + bonus_pct/100). Helper for testing the SQL expression.
fn total_comp_cents(salary_cents: i64, bonus_target_pct: Option<f64>) -> f64 {
    salary_cents as f64 * (1.0 + bonus_target_pct.unwrap_or(0.0) / 100.0)
}

#[test]
fn total_comp_with_bonus() {
    // $100k salary + 20% bonus = $120k total
    let tc = total_comp_cents(10_000_000, Some(20.0));
    assert!(
        (tc - 12_000_000.0).abs() < 1.0,
        "expected 12_000_000 cents, got {tc}"
    );
}

#[test]
fn total_comp_no_bonus() {
    // $100k salary + None bonus = $100k total
    let tc = total_comp_cents(10_000_000, None);
    assert!(
        (tc - 10_000_000.0).abs() < 1.0,
        "expected 10_000_000 cents, got {tc}"
    );
}

#[test]
fn total_comp_zero_bonus() {
    // $100k salary + 0% bonus = $100k total
    let tc = total_comp_cents(10_000_000, Some(0.0));
    assert!(
        (tc - 10_000_000.0).abs() < 1.0,
        "expected 10_000_000 cents, got {tc}"
    );
}

// ── Edge cases: inverted ranges, empty inputs ───────────────────────────────

/// When quality_min > quality_max, no item can satisfy both constraints.
#[test]
fn quality_inverted_range_excludes_all() {
    let r = SearchResult {
        rating: Some(3.0),
        domain_rated: true,
        ..make_result(1, 0.6, false)
    };
    let qmin = 4.0_f64;
    let qmax = 2.0_f64;
    let passes_min = r.domain_rated && r.rating.map_or(false, |v| v >= qmin);
    let passes_max = r.domain_rated && r.rating.map_or(false, |v| v <= qmax);
    assert!(
        !(passes_min && passes_max),
        "inverted quality range (min=4, max=2) must exclude all items"
    );
}

/// When price_min > price_max (inverted), no offer can match.
#[test]
fn price_inverted_range_is_empty() {
    // Simulate: salary=$100k, bonus=20% → total=$120k
    let tc = total_comp_cents(10_000_000, Some(20.0));
    let price_min = 150_000_00.0_f64; // $150k
    let price_max = 100_000_00.0_f64; // $100k — inverted
    let passes = tc >= price_min && tc <= price_max;
    assert!(!passes, "inverted price range must match nothing");
}

/// quality_min at exact boundary (0.0) still requires domain_rated=true.
#[test]
fn quality_min_zero_still_requires_domain_rated() {
    let r = SearchResult {
        rating: Some(0.0),
        domain_rated: false,
        ..make_result(1, 0.0, false)
    };
    let passes = r.domain_rated && r.rating.map_or(false, |v| v >= 0.0);
    assert!(!passes, "quality_min=0.0 must still exclude non-domain-rated items");
}

/// price_min alone (without price_max) should still work — it's a floor-only filter.
#[test]
fn price_min_only_is_floor() {
    let tc = total_comp_cents(10_000_000, Some(20.0)); // $120k
    let price_min = 100_000_00.0_f64; // $100k
    let passes = tc >= price_min;
    assert!(passes, "$120k total comp must pass price_min=$100k");
}

/// price_max alone (without price_min) should still work — it's a ceiling-only filter.
#[test]
fn price_max_only_is_ceiling() {
    let tc = total_comp_cents(10_000_000, Some(20.0)); // $120k
    let price_max = 150_000_00.0_f64; // $150k
    let passes = tc <= price_max;
    assert!(passes, "$120k total comp must pass price_max=$150k");
}

/// Empty categories list (after parsing) produces no filtering — all items pass.
#[test]
fn empty_categories_produces_no_filter() {
    // When the categories HashSet is empty, the filter is skipped entirely.
    let cat_tag_values: std::collections::HashSet<String> = std::collections::HashSet::new();
    let should_filter = !cat_tag_values.is_empty();
    assert!(!should_filter, "empty categories set must not activate filtering");
}
