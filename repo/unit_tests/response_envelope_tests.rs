/// Unit tests for ApiResponse and PaginatedEnvelope serialization.
/// No database required.
#[cfg(test)]
mod tests {
    use serde_json::{json, Value};
    use talentflow::shared::response::{
        ApiResponse, PaginatedEnvelope, PaginationMeta, ResponseMeta,
    };

    // ── ApiResponse ───────────────────────────────────────────────────────────

    #[test]
    fn api_response_ok_serializes_data_field() {
        let resp: ApiResponse<_> = ApiResponse::ok(json!({"id": "abc", "name": "Alice"}));
        let val: Value = serde_json::to_value(&resp).expect("serialization failed");
        assert_eq!(val["data"]["id"], "abc");
        assert_eq!(val["data"]["name"], "Alice");
    }

    #[test]
    fn api_response_ok_omits_meta_field_when_none() {
        let resp: ApiResponse<_> = ApiResponse::ok(42u32);
        let val: Value = serde_json::to_value(&resp).expect("serialization failed");
        // meta should be absent (skip_serializing_if = "Option::is_none")
        assert!(
            val.get("meta").is_none(),
            "meta key must be absent when None; got: {val}"
        );
    }

    #[test]
    fn api_response_with_meta_serializes_request_id() {
        let resp = ApiResponse {
            data: json!("payload"),
            meta: Some(ResponseMeta {
                request_id: Some("req-xyz-123".into()),
            }),
        };
        let val: Value = serde_json::to_value(&resp).expect("serialization failed");
        assert_eq!(val["meta"]["request_id"], "req-xyz-123");
        assert_eq!(val["data"], "payload");
    }

    #[test]
    fn api_response_meta_omits_request_id_when_none() {
        let resp: ApiResponse<u32> = ApiResponse {
            data: 0,
            meta: Some(ResponseMeta { request_id: None }),
        };
        let val: Value = serde_json::to_value(&resp).expect("serialization failed");
        // request_id should be absent inside meta
        assert!(
            val["meta"].get("request_id").is_none(),
            "request_id must be absent when None; got: {val}"
        );
    }

    #[test]
    fn api_response_data_can_be_string() {
        let resp = ApiResponse::ok("hello".to_string());
        let text = serde_json::to_string(&resp).expect("serialization failed");
        assert!(text.contains("\"data\":\"hello\""), "unexpected: {text}");
    }

    #[test]
    fn api_response_data_can_be_vec() {
        let resp = ApiResponse::ok(vec![1u32, 2, 3]);
        let val: Value = serde_json::to_value(&resp).expect("serialization failed");
        assert_eq!(val["data"], json!([1, 2, 3]));
    }

    // ── PaginatedEnvelope ─────────────────────────────────────────────────────

    #[test]
    fn paginated_envelope_serializes_data_pagination_fields() {
        let envelope = PaginatedEnvelope {
            data: vec![json!({"id": 1}), json!({"id": 2})],
            pagination: PaginationMeta {
                page: 1,
                per_page: 25,
                total: 42,
            },
            meta: None,
        };
        let val: Value = serde_json::to_value(&envelope).expect("serialization failed");

        // data array
        assert!(val["data"].is_array());
        assert_eq!(val["data"].as_array().unwrap().len(), 2);

        // pagination fields
        assert_eq!(val["pagination"]["page"], 1);
        assert_eq!(val["pagination"]["per_page"], 25);
        assert_eq!(val["pagination"]["total"], 42);
    }

    #[test]
    fn paginated_envelope_omits_meta_when_none() {
        let envelope: PaginatedEnvelope<u32> = PaginatedEnvelope {
            data: vec![],
            pagination: PaginationMeta {
                page: 1,
                per_page: 10,
                total: 0,
            },
            meta: None,
        };
        let val: Value = serde_json::to_value(&envelope).expect("serialization failed");
        assert!(
            val.get("meta").is_none(),
            "meta must be absent when None; got: {val}"
        );
    }

    #[test]
    fn paginated_envelope_includes_meta_when_present() {
        let envelope: PaginatedEnvelope<u32> = PaginatedEnvelope {
            data: vec![],
            pagination: PaginationMeta {
                page: 2,
                per_page: 50,
                total: 200,
            },
            meta: Some(ResponseMeta {
                request_id: Some("req-paginated".into()),
            }),
        };
        let val: Value = serde_json::to_value(&envelope).expect("serialization failed");
        assert_eq!(val["meta"]["request_id"], "req-paginated");
        assert_eq!(val["pagination"]["page"], 2);
        assert_eq!(val["pagination"]["per_page"], 50);
        assert_eq!(val["pagination"]["total"], 200);
    }

    #[test]
    fn paginated_envelope_empty_data_serializes_as_empty_array() {
        let envelope: PaginatedEnvelope<Value> = PaginatedEnvelope {
            data: vec![],
            pagination: PaginationMeta {
                page: 1,
                per_page: 10,
                total: 0,
            },
            meta: None,
        };
        let val: Value = serde_json::to_value(&envelope).expect("serialization failed");
        assert_eq!(val["data"], json!([]));
        assert_eq!(val["pagination"]["total"], 0);
    }

    #[test]
    fn paginated_envelope_total_can_be_large() {
        let envelope: PaginatedEnvelope<u32> = PaginatedEnvelope {
            data: vec![],
            pagination: PaginationMeta {
                page: 1,
                per_page: 25,
                total: 1_000_000,
            },
            meta: None,
        };
        let val: Value = serde_json::to_value(&envelope).expect("serialization failed");
        assert_eq!(val["pagination"]["total"], 1_000_000_i64);
    }

    // ── Round-trip via serde_json::to_string / from_str ───────────────────────

    #[test]
    fn api_response_round_trips_via_string() {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct Payload {
            value: u32,
        }

        let original = ApiResponse::ok(Payload { value: 99 });
        let serialized = serde_json::to_string(&original).expect("to_string failed");
        let parsed: Value = serde_json::from_str(&serialized).expect("from_str failed");
        assert_eq!(parsed["data"]["value"], 99);
    }
}
