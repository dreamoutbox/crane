// TEMP DISABLE

// #[test]
// fn test_record_type_display() {
//     assert_eq!(DnsRecordType::A.to_string(), "A");
//     assert_eq!(DnsRecordType::Cname.to_string(), "CNAME");
//     assert_eq!(DnsRecordType::Mx.to_string(), "MX");
//     assert_eq!(DnsRecordType::Txt.to_string(), "TXT");
// }

// #[test]
// fn test_create_record_serialization() {
//     let record = CreateDnsRecord {
//         record_type: DnsRecordType::A,
//         name: "test.example.com".to_string(),
//         content: Some("1.2.3.4".to_string()),
//         ttl: 3600,
//         proxied: Some(false),
//         priority: None,
//         comment: Some("Test record".to_string()),
//         tags: None,
//         data: None,
//     };

//     let json = serde_json::to_string(&record).unwrap();
//     assert!(json.contains("\"type\":\"A\""));
//     assert!(json.contains("\"name\":\"test.example.com\""));
//     assert!(json.contains("\"content\":\"1.2.3.4\""));
//     // priority should be omitted
//     assert!(!json.contains("\"priority\""));
// }

// #[test]
// fn test_patch_record_skips_none_fields() {
//     let patch = PatchDnsRecord {
//         content: Some("5.6.7.8".to_string()),
//         ..Default::default()
//     };

//     let json = serde_json::to_string(&patch).unwrap();
//     assert!(json.contains("\"content\":\"5.6.7.8\""));
//     // All other fields should be omitted
//     assert!(!json.contains("\"name\""));
//     assert!(!json.contains("\"type\""));
//     assert!(!json.contains("\"ttl\""));
// }

// #[test]
// fn test_list_params_serialization() {
//     let params = ListDnsRecordsParams {
//         name: Some("api.example.com".to_string()),
//         record_type: Some(DnsRecordType::A),
//         per_page: Some(50),
//         ..Default::default()
//     };

//     // serde_qs or reqwest query serialisation
//     let json = serde_json::to_value(&params).unwrap();
//     assert_eq!(json["name"], "api.example.com");
//     assert_eq!(json["type"], "A");
//     assert_eq!(json["per_page"], 50);
//     assert!(json.get("content").is_none() || json["content"].is_null());
// }
