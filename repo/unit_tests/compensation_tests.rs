/// Unit tests for CompensationData validation and encryption round-trip.
use talentflow::domain::offers::models::CompensationData;

fn valid_compensation() -> CompensationData {
    CompensationData {
        base_salary_usd: 100_000,
        bonus_target_pct: 10.0,
        equity_units: 500,
        pto_days: 20,
        k401_match_pct: 5.0,
    }
}

#[test]
fn valid_compensation_passes_validation() {
    let errors = valid_compensation().validate();
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn zero_base_salary_fails() {
    let c = CompensationData {
        base_salary_usd: 0,
        ..valid_compensation()
    };
    let errors = c.validate();
    assert!(!errors.is_empty());
    assert!(errors[0].contains("base_salary_usd"));
}

#[test]
fn bonus_pct_over_100_fails() {
    let c = CompensationData {
        bonus_target_pct: 100.1,
        ..valid_compensation()
    };
    let errors = c.validate();
    assert!(!errors.is_empty());
    assert!(errors[0].contains("bonus_target_pct"));
}

#[test]
fn negative_bonus_pct_fails() {
    let c = CompensationData {
        bonus_target_pct: -0.1,
        ..valid_compensation()
    };
    let errors = c.validate();
    assert!(!errors.is_empty());
}

#[test]
fn pto_days_over_365_fails() {
    let c = CompensationData {
        pto_days: 366,
        ..valid_compensation()
    };
    let errors = c.validate();
    assert!(!errors.is_empty());
    assert!(errors[0].contains("pto_days"));
}

#[test]
fn k401_match_over_100_fails() {
    let c = CompensationData {
        k401_match_pct: 101.0,
        ..valid_compensation()
    };
    let errors = c.validate();
    assert!(!errors.is_empty());
    assert!(errors[0].contains("k401_match_pct"));
}

#[test]
fn multiple_errors_collected() {
    let c = CompensationData {
        base_salary_usd: 0,
        bonus_target_pct: 200.0,
        pto_days: 400,
        k401_match_pct: -5.0,
        ..valid_compensation()
    };
    let errors = c.validate();
    assert!(errors.len() >= 3, "expected ≥3 errors, got: {errors:?}");
}

#[test]
fn boundary_values_pass() {
    let c = CompensationData {
        base_salary_usd: 1,
        bonus_target_pct: 0.0,
        equity_units: 0,
        pto_days: 365,
        k401_match_pct: 100.0,
    };
    assert!(c.validate().is_empty());
}

#[test]
fn compensation_serializes_and_deserializes() {
    let original = valid_compensation();
    let json = serde_json::to_vec(&original).unwrap();
    let restored: CompensationData = serde_json::from_slice(&json).unwrap();
    assert_eq!(original.base_salary_usd, restored.base_salary_usd);
    assert_eq!(original.bonus_target_pct, restored.bonus_target_pct);
    assert_eq!(original.equity_units, restored.equity_units);
    assert_eq!(original.pto_days, restored.pto_days);
    assert_eq!(original.k401_match_pct, restored.k401_match_pct);
}
