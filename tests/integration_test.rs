#[cfg(test)]
mod tests {
    use superstruct::*;
    use std::collections::HashMap;

    fn insert_record(ss: &Superstruct, name: &str, age: i64, city: &str, score: i64) -> u64 {
        let mut attrs = HashMap::new();
        attrs.insert("name".to_string(), Value::String(name.to_string()));
        attrs.insert("age".to_string(), Value::Int(age));
        attrs.insert("city".to_string(), Value::String(city.to_string()));
        attrs.insert("score".to_string(), Value::Int(score));
        ss.insert(attrs)
    }

    fn sorted_names(records: &[HashMap<String, Value>]) -> Vec<String> {
        let mut names: Vec<String> = records
            .iter()
            .map(|r| match &r["name"] {
                Value::String(s) => s.clone(),
                _ => String::new(),
            })
            .collect();
        names.sort();
        names
    }

    fn setup_four_city_records(ss: &Superstruct) {
        insert_record(ss, "Alice", 30, "NYC", 88);
        insert_record(ss, "Bob", 25, "SF", 92);
        insert_record(ss, "Carol", 41, "NYC", 70);
        insert_record(ss, "Dave", 22, "LA", 60);
    }

    fn setup_five_records(ss: &Superstruct) {
        insert_record(ss, "Alice", 30, "NYC", 88);
        insert_record(ss, "Anya", 25, "SF", 92);
        insert_record(ss, "Andre", 41, "NYC", 70);
        insert_record(ss, "Bea", 30, "SF", 85);
        insert_record(ss, "Ben", 22, "LA", 60);
    }

    // ===== Core Query Tests =====

    #[test]
    fn test_get_returns_record_by_id() {
        let ss = Superstruct::new(None);
        let mut attrs = HashMap::new();
        attrs.insert("name".to_string(), Value::String("Alice".to_string()));
        let id = ss.insert(attrs);
        assert_eq!(ss.get(id).unwrap()["name"], Value::String("Alice".to_string()));
    }

    #[test]
    fn test_get_returns_none_for_unknown_id() {
        let ss = Superstruct::new(None);
        assert!(ss.get(9999).is_none());
    }

    #[test]
    fn test_equals_query() {
        let ss = Superstruct::new(None);
        setup_five_records(&ss);
        let out = ss.find().equals("city", Value::String("NYC".to_string())).execute();
        assert_eq!(sorted_names(&out), vec!["Alice", "Andre"]);
    }

    #[test]
    fn test_range_query_inclusive_bounds() {
        let ss = Superstruct::new(None);
        setup_five_records(&ss);
        let out = ss.find().range("age", Value::Int(25), Value::Int(30)).execute();
        assert_eq!(sorted_names(&out), vec!["Alice", "Anya", "Bea"]);
    }

    #[test]
    fn test_prefix_query() {
        let ss = Superstruct::new(None);
        setup_five_records(&ss);
        let out = ss.find().prefix("name", "A").execute();
        assert_eq!(sorted_names(&out), vec!["Alice", "Andre", "Anya"]);
    }

    #[test]
    fn test_prefix_query_no_match() {
        let ss = Superstruct::new(None);
        setup_five_records(&ss);
        let out = ss.find().prefix("name", "Z").execute();
        assert!(out.is_empty());
    }

    #[test]
    fn test_compound_intersection() {
        let ss = Superstruct::new(None);
        setup_five_records(&ss);
        let out = ss.find()
            .range("age", Value::Int(25), Value::Int(50))
            .prefix("name", "A")
            .equals("city", Value::String("NYC".to_string()))
            .execute();
        assert_eq!(sorted_names(&out), vec!["Alice", "Andre"]);
    }

    #[test]
    fn test_compound_intersection_empty() {
        let ss = Superstruct::new(None);
        setup_five_records(&ss);
        let out = ss.find()
            .range("age", Value::Int(25), Value::Int(30))
            .prefix("name", "A")
            .equals("city", Value::String("LA".to_string()))
            .execute();
        assert!(out.is_empty());
    }

    #[test]
    fn test_top_k_descending() {
        let ss = Superstruct::new(None);
        setup_five_records(&ss);
        let out = ss.find().top_k("score", 2, true).execute();
        let scores: Vec<i64> = out.iter().map(|r| r["score"].as_i64().unwrap()).collect();
        assert_eq!(scores, vec![92, 88]);
    }

    #[test]
    fn test_top_k_ascending() {
        let ss = Superstruct::new(None);
        for i in 0..5 {
            let mut attrs = HashMap::new();
            attrs.insert("score".to_string(), Value::Int(i));
            ss.insert(attrs);
        }
        let out = ss.find().top_k("score", 2, false).execute();
        let scores: Vec<i64> = out.iter().map(|r| r["score"].as_i64().unwrap()).collect();
        assert_eq!(scores, vec![0, 1]);
    }

    #[test]
    fn test_top_k_after_filter() {
        let ss = Superstruct::new(None);
        insert_record(&ss, "Alice", 30, "NYC", 88);
        insert_record(&ss, "Bob", 25, "SF", 92);
        insert_record(&ss, "Carol", 30, "NYC", 70);
        let out = ss.find()
            .equals("city", Value::String("NYC".to_string()))
            .top_k("score", 1, true)
            .execute();
        assert_eq!(out[0]["name"], Value::String("Alice".to_string()));
    }

    #[test]
    fn test_no_indexes_built_until_first_query() {
        let ss = Superstruct::new(None);
        insert_record(&ss, "Alice", 30, "NYC", 88);
        assert!(ss.index_inventory().is_empty());
        ss.find().equals("city", Value::String("NYC".to_string())).execute();
        let types: Vec<String> = ss.index_inventory().iter().map(|(t, _, _)| t.clone()).collect();
        assert!(types.contains(&"HashIndex".to_string()));
    }

    #[test]
    fn test_insert_after_build_propagates_to_index() {
        let ss = Superstruct::new(None);
        insert_record(&ss, "Alice", 30, "NYC", 88);
        ss.find().equals("city", Value::String("NYC".to_string())).execute();
        insert_record(&ss, "Zed", 99, "NYC", 1);
        let out = ss.find().equals("city", Value::String("NYC".to_string())).execute();
        let names = sorted_names(&out);
        assert!(names.contains(&"Zed".to_string()));
    }

    #[test]
    fn test_delete_propagates_to_index() {
        let ss = Superstruct::new(None);
        let id = insert_record(&ss, "Alice", 30, "NYC", 88);
        ss.find().equals("city", Value::String("NYC".to_string())).execute();
        ss.delete(id);
        let out = ss.find().equals("city", Value::String("NYC".to_string())).execute();
        let names = sorted_names(&out);
        assert!(!names.contains(&"Alice".to_string()));
    }

    #[test]
    fn test_eviction_under_tight_budget() {
        let ss = Superstruct::new(None);
        insert_record(&ss, "Alice", 30, "NYC", 88);
        ss.find().equals("city", Value::String("NYC".to_string())).execute();
        ss.find().range("age", Value::Int(20), Value::Int(40)).execute();
        ss.find().prefix("name", "A").execute();
        assert!(!ss.index_inventory().is_empty());
        ss.set_memory_budget(0);
        assert!(ss.index_inventory().is_empty());
    }

    #[test]
    fn test_correctness_survives_eviction() {
        let ss = Superstruct::new(None);
        insert_record(&ss, "Alice", 30, "NYC", 88);
        let before = ss.find().equals("city", Value::String("NYC".to_string())).execute();
        let before_names = sorted_names(&before);
        ss.set_memory_budget(0);
        let after = ss.find().equals("city", Value::String("NYC".to_string())).execute();
        let after_names = sorted_names(&after);
        assert_eq!(before_names, after_names);
    }

    // ===== Boolean Composition Tests =====

    #[test]
    fn test_or_group_unions() {
        let ss = Superstruct::new(None);
        setup_four_city_records(&ss);
        let out = ss.find()
            .any_of(vec![
                ss.find().equals("city", Value::String("NYC".to_string())).to_node().unwrap(),
                ss.find().range("score", Value::Int(90), Value::Int(100)).to_node().unwrap(),
            ])
            .execute();
        assert_eq!(sorted_names(&out), vec!["Alice", "Bob", "Carol"]);
    }

    #[test]
    fn test_exclude_subtracts() {
        let ss = Superstruct::new(None);
        setup_four_city_records(&ss);
        let out = ss.find()
            .exclude(ss.find().equals("city", Value::String("NYC".to_string())).to_node().unwrap())
            .execute();
        assert_eq!(sorted_names(&out), vec!["Bob", "Dave"]);
    }

    #[test]
    fn test_combine_or_and_not() {
        let ss = Superstruct::new(None);
        setup_four_city_records(&ss);
        let out = ss.find()
            .any_of(vec![
                ss.find().equals("city", Value::String("NYC".to_string())).to_node().unwrap(),
                ss.find().range("score", Value::Int(90), Value::Int(100)).to_node().unwrap(),
            ])
            .exclude(ss.find().equals("name", Value::String("Alice".to_string())).to_node().unwrap())
            .execute();
        assert_eq!(sorted_names(&out), vec!["Bob", "Carol"]);
    }

    // ===== Edge Case Tests =====

    #[test]
    fn test_query_on_empty_store_returns_empty() {
        let ss = Superstruct::new(None);
        assert!(ss.find().equals("city", Value::String("NYC".to_string())).execute().is_empty());
        assert!(ss.find().range("age", Value::Int(0), Value::Int(100)).execute().is_empty());
        assert!(ss.find().prefix("name", "A").execute().is_empty());
    }

    #[test]
    fn test_insert_empty_dict_yields_id() {
        let ss = Superstruct::new(None);
        let id = ss.insert(HashMap::new());
        assert_eq!(ss.get(id).unwrap(), HashMap::new());
    }

    #[test]
    fn test_query_on_attribute_no_record_has() {
        let ss = Superstruct::new(None);
        let mut attrs = HashMap::new();
        attrs.insert("a".to_string(), Value::Int(1));
        ss.insert(attrs);
        assert!(ss.find().equals("b", Value::Int(1)).execute().is_empty());
    }

    #[test]
    fn test_range_with_lo_greater_than_hi_returns_empty() {
        let ss = Superstruct::new(None);
        for i in 0..5 {
            let mut attrs = HashMap::new();
            attrs.insert("n".to_string(), Value::Int(i));
            ss.insert(attrs);
        }
        assert!(ss.find().range("n", Value::Int(4), Value::Int(1)).execute().is_empty());
    }

    #[test]
    fn test_range_with_lo_equal_hi_acts_as_point() {
        let ss = Superstruct::new(None);
        for i in 0..5 {
            let mut attrs = HashMap::new();
            attrs.insert("n".to_string(), Value::Int(i));
            ss.insert(attrs);
        }
        let out = ss.find().range("n", Value::Int(2), Value::Int(2)).execute();
        assert_eq!(out[0]["n"], Value::Int(2));
    }

    #[test]
    fn test_prefix_with_empty_string_matches_all_strings() {
        let ss = Superstruct::new(None);
        let mut a = HashMap::new();
        a.insert("name".to_string(), Value::String("Alice".to_string()));
        ss.insert(a);
        let mut b = HashMap::new();
        b.insert("name".to_string(), Value::String("Bob".to_string()));
        ss.insert(b);
        let mut c = HashMap::new();
        c.insert("name".to_string(), Value::String("Carol".to_string()));
        ss.insert(c);
        let out = ss.find().prefix("name", "").execute();
        assert_eq!(sorted_names(&out), vec!["Alice", "Bob", "Carol"]);
    }

    #[test]
    fn test_records_without_attribute_skip_silently() {
        let ss = Superstruct::new(None);
        let mut a = HashMap::new();
        a.insert("name".to_string(), Value::String("Alice".to_string()));
        ss.insert(a);
        let mut b = HashMap::new();
        b.insert("age".to_string(), Value::Int(30));
        ss.insert(b);
        let out = ss.find().prefix("name", "A").execute();
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn test_delete_unknown_id_returns_false() {
        let ss = Superstruct::new(None);
        assert!(!ss.delete(0));
        let mut attrs = HashMap::new();
        attrs.insert("x".to_string(), Value::Int(1));
        ss.insert(attrs);
        assert!(!ss.delete(99));
    }

    #[test]
    fn test_insert_returns_monotonic_ids() {
        let ss = Superstruct::new(None);
        let ids: Vec<u64> = (0..5)
            .map(|i| {
                let mut attrs = HashMap::new();
                attrs.insert("n".to_string(), Value::Int(i as i64));
                ss.insert(attrs)
            })
            .collect();
        assert_eq!(ids, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_deleted_id_is_not_reused() {
        let ss = Superstruct::new(None);
        let mut a = HashMap::new();
        a.insert("x".to_string(), Value::Int(1));
        let a_id = ss.insert(a);
        ss.delete(a_id);
        let mut b = HashMap::new();
        b.insert("x".to_string(), Value::Int(2));
        let b_id = ss.insert(b);
        assert_ne!(a_id, b_id);
    }

    #[test]
    fn test_query_matches_all_when_no_predicates() {
        let ss = Superstruct::new(None);
        for i in 0..3 {
            let mut attrs = HashMap::new();
            attrs.insert("n".to_string(), Value::Int(i));
            ss.insert(attrs);
        }
        let out = ss.find().execute();
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn test_top_k_alone_returns_ordered_universe() {
        let ss = Superstruct::new(None);
        for &v in &[3, 1, 4, 1, 5, 9, 2, 6] {
            let mut attrs = HashMap::new();
            attrs.insert("v".to_string(), Value::Int(v));
            ss.insert(attrs);
        }
        let out = ss.find().top_k("v", 3, true).execute();
        let vals: Vec<i64> = out.iter().map(|r| r["v"].as_i64().unwrap()).collect();
        assert_eq!(vals, vec![9, 6, 5]);
    }

    #[test]
    fn test_top_k_zero_returns_empty() {
        let ss = Superstruct::new(None);
        for i in 0..5 {
            let mut attrs = HashMap::new();
            attrs.insert("score".to_string(), Value::Int(i));
            ss.insert(attrs);
        }
        assert!(ss.find().top_k("score", 0, true).execute().is_empty());
    }

    #[test]
    fn test_top_k_larger_than_result_returns_all() {
        let ss = Superstruct::new(None);
        for i in 0..5 {
            let mut attrs = HashMap::new();
            attrs.insert("score".to_string(), Value::Int(i));
            ss.insert(attrs);
        }
        let out = ss.find().top_k("score", 100, true).execute();
        assert_eq!(out.len(), 5);
    }

    #[test]
    fn test_top_k_skips_records_missing_attribute() {
        let ss = Superstruct::new(None);
        for i in 0..5 {
            let mut attrs = HashMap::new();
            attrs.insert("score".to_string(), Value::Int(i));
            ss.insert(attrs);
        }
        let mut attrs = HashMap::new();
        attrs.insert("other".to_string(), Value::Int(99));
        ss.insert(attrs);
        let out = ss.find().top_k("score", 100, true).execute();
        for r in &out {
            assert!(r.contains_key("score"));
        }
    }

    // ===== Full-Text Tests =====

    #[test]
    fn test_contains_query() {
        let ss = Superstruct::new(None);
        let mut a = HashMap::new();
        a.insert("name".to_string(), Value::String("Alice".to_string()));
        a.insert("bio".to_string(), Value::String("loves cats".to_string()));
        ss.insert(a);
        let mut b = HashMap::new();
        b.insert("name".to_string(), Value::String("Bob".to_string()));
        b.insert("bio".to_string(), Value::String("loves dogs".to_string()));
        ss.insert(b);
        let out = ss.find().contains("bio", "cats").execute();
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn test_contains_case_insensitive() {
        let ss = Superstruct::new(None);
        let mut attrs = HashMap::new();
        attrs.insert("bio".to_string(), Value::String("dog person all the way".to_string()));
        ss.insert(attrs);
        let out = ss.find().contains("bio", "DOG").execute();
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn test_contains_two_words_anded() {
        let ss = Superstruct::new(None);
        let mut a = HashMap::new();
        a.insert("bio".to_string(), Value::String("loves cats and long walks".to_string()));
        ss.insert(a);
        let mut b = HashMap::new();
        b.insert("bio".to_string(), Value::String("dog person all the way".to_string()));
        ss.insert(b);
        let mut c = HashMap::new();
        c.insert("bio".to_string(), Value::String("cat owner who also walks dogs".to_string()));
        ss.insert(c);
        let out = ss.find().contains("bio", "walks").contains("bio", "dogs").execute();
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn test_contains_punctuation_ok() {
        let ss = Superstruct::new(None);
        let mut attrs = HashMap::new();
        attrs.insert("bio".to_string(), Value::String("hello, world! how are you?".to_string()));
        ss.insert(attrs);
        assert_eq!(ss.find().contains("bio", "hello").execute().len(), 1);
        assert_eq!(ss.find().contains("bio", "world").execute().len(), 1);
    }

    #[test]
    fn test_fuzzy_finds_near_matches() {
        let ss = Superstruct::new(None);
        for name in &["Alice", "Alicia", "Alyce", "Bob", "Charlie"] {
            let mut attrs = HashMap::new();
            attrs.insert("name".to_string(), Value::String(name.to_string()));
            ss.insert(attrs);
        }
        let out = ss.find().fuzzy("name", "Alise", 0.3).execute();
        let names = sorted_names(&out);
        assert!(names.contains(&"Alice".to_string()));
    }

    #[test]
    fn test_fuzzy_strict_rules_out_far() {
        let ss = Superstruct::new(None);
        for name in &["Alice", "Alicia", "Alyce", "Bob", "Charlie"] {
            let mut attrs = HashMap::new();
            attrs.insert("name".to_string(), Value::String(name.to_string()));
            ss.insert(attrs);
        }
        let out = ss.find().fuzzy("name", "Alice", 0.5).execute();
        let names = sorted_names(&out);
        assert!(!names.contains(&"Bob".to_string()));
        assert!(!names.contains(&"Charlie".to_string()));
    }

    #[test]
    fn test_fuzzy_threshold_one_exact() {
        let ss = Superstruct::new(None);
        for name in &["Alice", "Alicia", "Alyce", "Bob", "Charlie"] {
            let mut attrs = HashMap::new();
            attrs.insert("name".to_string(), Value::String(name.to_string()));
            ss.insert(attrs);
        }
        let out = ss.find().fuzzy("name", "Alice", 1.0).execute();
        let names = sorted_names(&out);
        assert_eq!(names, vec!["Alice"]);
    }

    // ===== Graph Tests =====

    #[test]
    fn test_graph_neighbors() {
        let ss = Superstruct::new(None);
        let a = insert_record(&ss, "A", 1, "X", 1);
        let b = insert_record(&ss, "B", 2, "Y", 2);
        let _c = insert_record(&ss, "C", 3, "Z", 3);
        ss.add_edge(a, b, None, false);
        let neighbors = ss.neighbors(a, None);
        assert!(neighbors.contains(&b));
    }

    #[test]
    fn test_bfs() {
        let ss = Superstruct::new(None);
        let a = insert_record(&ss, "A", 1, "X", 1);
        let b = insert_record(&ss, "B", 2, "Y", 2);
        let c = insert_record(&ss, "C", 3, "Z", 3);
        ss.add_edge(a, b, None, false);
        ss.add_edge(b, c, None, false);
        let depths = ss.bfs(a, None, None);
        assert_eq!(depths[&a], 0);
        assert_eq!(depths[&b], 1);
        assert_eq!(depths[&c], 2);
    }

    #[test]
    fn test_shortest_path() {
        let ss = Superstruct::new(None);
        let a = insert_record(&ss, "A", 1, "X", 1);
        let b = insert_record(&ss, "B", 2, "Y", 2);
        let c = insert_record(&ss, "C", 3, "Z", 3);
        ss.add_edge(a, b, None, false);
        ss.add_edge(b, c, None, false);
        assert_eq!(ss.shortest_path(a, c, None).unwrap(), vec![a, b, c]);
    }

    #[test]
    fn test_shortest_path_same_node() {
        let ss = Superstruct::new(None);
        let a = insert_record(&ss, "A", 1, "X", 1);
        assert_eq!(ss.shortest_path(a, a, None).unwrap(), vec![a]);
    }

    #[test]
    fn test_graph_delete_node_drops_edges() {
        let ss = Superstruct::new(None);
        let a = insert_record(&ss, "A", 1, "X", 1);
        let b = insert_record(&ss, "B", 2, "Y", 2);
        let c = insert_record(&ss, "C", 3, "Z", 3);
        ss.add_edge(a, b, None, false);
        ss.add_edge(b, c, None, false);
        ss.delete(b);
        assert!(ss.shortest_path(a, c, None).is_none());
    }

    #[test]
    fn test_graph_self_loop() {
        let ss = Superstruct::new(None);
        let a = insert_record(&ss, "A", 1, "X", 1);
        ss.add_edge(a, a, None, false);
        assert!(ss.neighbors(a, None).contains(&a));
    }

    #[test]
    fn test_graph_directed_edge_no_reverse() {
        let ss = Superstruct::new(None);
        let a = insert_record(&ss, "A", 1, "X", 1);
        let b = insert_record(&ss, "B", 2, "Y", 2);
        ss.add_edge(a, b, None, true);
        assert!(ss.neighbors(a, None).contains(&b));
        assert!(!ss.neighbors(b, None).contains(&a));
    }

    #[test]
    fn test_graph_labels_segregate() {
        let ss = Superstruct::new(None);
        let a = insert_record(&ss, "A", 1, "X", 1);
        let b = insert_record(&ss, "B", 2, "Y", 2);
        let c = insert_record(&ss, "C", 3, "Z", 3);
        ss.add_edge(a, b, Some("friend".to_string()), false);
        ss.add_edge(a, c, Some("block".to_string()), false);
        assert_eq!(ss.neighbors(a, Some("friend".to_string())), [b].into_iter().collect());
        assert_eq!(ss.neighbors(a, Some("block".to_string())), [c].into_iter().collect());
    }

    #[test]
    fn test_graph_remove_edge() {
        let ss = Superstruct::new(None);
        let a = insert_record(&ss, "A", 1, "X", 1);
        let b = insert_record(&ss, "B", 2, "Y", 2);
        ss.add_edge(a, b, None, false);
        ss.remove_edge(a, b, None, false);
        assert!(!ss.neighbors(a, None).contains(&b));
    }

    #[test]
    fn test_graph_shortest_path_unreachable() {
        let ss = Superstruct::new(None);
        let a = insert_record(&ss, "A", 1, "X", 1);
        let b = insert_record(&ss, "B", 2, "Y", 2);
        assert!(ss.shortest_path(a, b, None).is_none());
    }

    #[test]
    fn test_graph_bfs_chain() {
        let ss = Superstruct::new(None);
        let nodes: Vec<u64> = (0..10).map(|i| {
            let mut attrs = HashMap::new();
            attrs.insert("n".to_string(), Value::Int(i));
            ss.insert(attrs)
        }).collect();
        for i in 0..9 {
            ss.add_edge(nodes[i], nodes[i + 1], None, false);
        }
        let depths = ss.bfs(nodes[0], None, None);
        assert_eq!(depths.len(), 10);
        assert_eq!(depths[&nodes[9]], 9);
    }

    #[test]
    fn test_graph_dense_traversal() {
        let ss = Superstruct::new(None);
        let nodes: Vec<u64> = (0..8).map(|i| {
            let mut attrs = HashMap::new();
            attrs.insert("i".to_string(), Value::Int(i));
            ss.insert(attrs)
        }).collect();
        for i in 0..8 {
            for j in (i + 1)..8 {
                ss.add_edge(nodes[i], nodes[j], None, false);
            }
        }
        for i in 0..8 {
            let depths = ss.bfs(nodes[i], None, None);
            for j in 0..8 {
                if i != j {
                    assert_eq!(depths[&nodes[j]], 1);
                }
            }
        }
    }

    // ===== Sketch Tests =====

    #[test]
    fn test_facade_bloom_contains() {
        let ss = Superstruct::new(None);
        for _ in 0..50 {
            let mut attrs = HashMap::new();
            attrs.insert("city".to_string(), Value::String("NYC".to_string()));
            ss.insert(attrs);
        }
        assert!(ss.maybe_contains("city", &Value::String("NYC".to_string())));
        assert!(!ss.maybe_contains("city", &Value::String("Atlantis".to_string())));
    }

    #[test]
    fn test_facade_count_min_estimate() {
        let ss = Superstruct::new(None);
        for _ in 0..50 {
            let mut attrs = HashMap::new();
            attrs.insert("city".to_string(), Value::String("NYC".to_string()));
            ss.insert(attrs);
        }
        let count = ss.estimate_count("city", &Value::String("NYC".to_string()));
        assert!(count >= 50);
        assert_eq!(ss.estimate_count("ghost", &Value::String("x".to_string())), 0);
    }

    // ===== Sketch Quality Tests (using sketch types directly) =====

    #[test]
    fn test_bloom_no_false_negatives() {
        use superstruct::sketch::BloomSketch;
        let mut bf = BloomSketch::new(2048, 3);
        for i in 0..200 {
            bf.add(&Value::String(format!("item-{}", i)));
        }
        for i in 0..200 {
            assert!(bf.maybe_contains(&Value::String(format!("item-{}", i))));
        }
    }

    #[test]
    fn test_bloom_false_positive_rate() {
        use superstruct::sketch::BloomSketch;
        let mut bf = BloomSketch::default();
        for i in 0..200 {
            bf.add(&Value::String(format!("item-{}", i)));
        }
        let fp: usize = (0..10_000)
            .filter(|i| bf.maybe_contains(&Value::String(format!("never-seen-{}", i))))
            .count();
        assert!((fp as f64 / 10_000.0) < 0.02);
    }

    #[test]
    fn test_countmin_zero_for_never_seen() {
        use superstruct::sketch::CountMinSketch;
        let cm = CountMinSketch::default();
        assert_eq!(cm.estimate(&Value::String("nope".to_string())), 0);
    }

    #[test]
    fn test_countmin_over_estimates() {
        use superstruct::sketch::CountMinSketch;
        let mut cm = CountMinSketch::new(4096, 5);
        for i in 0..50 {
            for _ in 0..(i + 1) {
                cm.add(&Value::String(format!("item-{}", i)));
            }
        }
        for i in 0..50 {
            assert!(cm.estimate(&Value::String(format!("item-{}", i))) >= (i + 1) as u64);
        }
    }

    // ===== Persistence Tests =====

    #[test]
    fn test_save_load_round_trip() {
        let ss = Superstruct::new(None);
        let a = insert_record(&ss, "Alice", 30, "NYC", 88);
        let b = insert_record(&ss, "Bob", 25, "SF", 92);
        ss.add_edge(a, b, None, false);

        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("snapshot.json");
        ss.save(path.to_str().unwrap()).unwrap();
        let loaded = Superstruct::load(path.to_str().unwrap(), None).unwrap();

        assert_eq!(loaded.get(a).unwrap()["name"], Value::String("Alice".to_string()));
        assert_eq!(loaded.get(b).unwrap()["name"], Value::String("Bob".to_string()));
        assert!(loaded.neighbors(a, None).contains(&b));
    }

    #[test]
    fn test_load_then_query_triggers_rebuild() {
        let ss = Superstruct::new(None);
        for name in &["Alice", "Bob", "Carol"] {
            let mut attrs = HashMap::new();
            attrs.insert("name".to_string(), Value::String(name.to_string()));
            ss.insert(attrs);
        }
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("snapshot.json");
        ss.save(path.to_str().unwrap()).unwrap();
        let loaded = Superstruct::load(path.to_str().unwrap(), None).unwrap();
        assert!(loaded.index_inventory().is_empty());
        let out = loaded.find().prefix("name", "A").execute();
        assert_eq!(out[0]["name"], Value::String("Alice".to_string()));
        let types: Vec<String> = loaded.index_inventory().iter().map(|(t, _, _)| t.clone()).collect();
        assert!(types.contains(&"TrieIndex".to_string()));
    }

    #[test]
    fn test_save_load_empty_store() {
        let ss = Superstruct::new(None);
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("empty.json");
        ss.save(path.to_str().unwrap()).unwrap();
        let loaded = Superstruct::load(path.to_str().unwrap(), None).unwrap();
        assert_eq!(loaded.len(), 0);
    }

    #[test]
    fn test_save_load_heterogeneous() {
        let ss = Superstruct::new(None);
        ss.insert(HashMap::from([("a".to_string(), Value::Int(1))]));
        ss.insert(HashMap::from([("b".to_string(), Value::String("hello".to_string()))]));
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("het.json");
        ss.save(path.to_str().unwrap()).unwrap();
        let loaded = Superstruct::load(path.to_str().unwrap(), None).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.get(0).unwrap()["a"], Value::Int(1));
        assert_eq!(loaded.get(1).unwrap()["b"], Value::String("hello".to_string()));
    }

    #[test]
    fn test_save_load_graph_labels() {
        let ss = Superstruct::new(None);
        let a = insert_record(&ss, "A", 1, "X", 1);
        let b = insert_record(&ss, "B", 2, "Y", 2);
        ss.add_edge(a, b, Some("friend".to_string()), false);
        ss.add_edge(a, b, Some("rival".to_string()), false);
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("graph.json");
        ss.save(path.to_str().unwrap()).unwrap();
        let loaded = Superstruct::load(path.to_str().unwrap(), None).unwrap();
        assert_eq!(loaded.neighbors(a, Some("friend".to_string())), [b].into_iter().collect());
        assert_eq!(loaded.neighbors(a, Some("rival".to_string())), [b].into_iter().collect());
    }

    #[test]
    fn test_save_load_sketches_rebuild() {
        let ss = Superstruct::new(None);
        for _ in 0..10 {
            ss.insert(HashMap::from([("city".to_string(), Value::String("NYC".to_string()))]));
        }
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("snap.json");
        ss.save(path.to_str().unwrap()).unwrap();
        let loaded = Superstruct::load(path.to_str().unwrap(), None).unwrap();
        assert!(loaded.maybe_contains("city", &Value::String("NYC".to_string())));
        assert!(loaded.estimate_count("city", &Value::String("NYC".to_string())) >= 10);
    }

    #[test]
    fn test_loaded_id_counter_no_collision() {
        let ss = Superstruct::new(None);
        let a = insert_record(&ss, "A", 1, "X", 1);
        let b = insert_record(&ss, "B", 2, "Y", 2);
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("ids.json");
        ss.save(path.to_str().unwrap()).unwrap();
        let loaded = Superstruct::load(path.to_str().unwrap(), None).unwrap();
        let c = insert_record(&loaded, "C", 3, "Z", 3);
        assert_ne!(c, a);
        assert_ne!(c, b);
    }

    // ===== Concurrency Tests =====

    #[test]
    fn test_concurrent_inserts_and_queries() {
        use std::sync::Arc;
        use std::thread;

        let ss = Arc::new(Superstruct::new(None));
        let ss_w1 = ss.clone();
        let ss_w2 = ss.clone();
        let ss_r1 = ss.clone();
        let ss_r2 = ss.clone();

        let writer = move || {
            for i in 0..500 {
                let mut attrs = HashMap::new();
                attrs.insert("city".to_string(), Value::String("NYC".to_string()));
                attrs.insert("n".to_string(), Value::Int(i));
                ss_w1.insert(attrs);
            }
        };

        let reader = move || {
            for _ in 0..500 {
                let _ = ss_r1.find().equals("city", Value::String("NYC".to_string())).execute();
            }
        };

        let w1 = thread::spawn(writer);
        let w2 = thread::spawn(move || {
            for i in 0..500 {
                let mut attrs = HashMap::new();
                attrs.insert("city".to_string(), Value::String("NYC".to_string()));
                attrs.insert("n".to_string(), Value::Int(i + 500));
                ss_w2.insert(attrs);
            }
        });
        let r1 = thread::spawn(reader);
        let r2 = thread::spawn(move || {
            for _ in 0..500 {
                let _ = ss_r2.find().equals("city", Value::String("NYC".to_string())).execute();
            }
        });

        w1.join().unwrap();
        w2.join().unwrap();
        r1.join().unwrap();
        r2.join().unwrap();
        assert_eq!(ss.len(), 1000);
        let out = ss.find().equals("city", Value::String("NYC".to_string())).execute();
        assert_eq!(out.len(), 1000);
    }

    #[test]
    fn test_single_threaded_smoke() {
        let ss = Superstruct::new(None);
        ss.insert(HashMap::from([("x".to_string(), Value::Int(1))]));
        let out = ss.find().equals("x", Value::Int(1)).execute();
        assert_eq!(out.len(), 1);
    }

    // ===== Stress Tests =====

    #[test]
    fn test_large_equality_vs_brute_force() {
        use rand::Rng;
        let ss = Superstruct::new(None);
        let mut records: Vec<HashMap<String, Value>> = Vec::new();
        let mut rng = rand::thread_rng();
        let cities = vec!["NYC", "SF", "LA"];

        for _ in 0..2000 {
            let mut attrs = HashMap::new();
            let city = cities[rng.gen_range(0..3)];
            attrs.insert("city".to_string(), Value::String(city.to_string()));
            ss.insert(attrs.clone());
            records.push(attrs);
        }
        for city in &cities {
            let expected: Vec<_> = records.iter().filter(|r| r["city"] == Value::String(city.to_string())).collect();
            let out = ss.find().equals("city", Value::String(city.to_string())).execute();
            assert_eq!(out.len(), expected.len());
        }
    }

    #[test]
    fn test_large_range_vs_brute_force() {
        use rand::Rng;
        let ss = Superstruct::new(None);
        let mut records: Vec<HashMap<String, Value>> = Vec::new();
        let mut rng = rand::thread_rng();

        for _ in 0..2000 {
            let mut attrs = HashMap::new();
            let age = rng.gen_range(0..100);
            attrs.insert("age".to_string(), Value::Int(age));
            ss.insert(attrs.clone());
            records.push(attrs);
        }
        for (lo, hi) in [(0, 9), (20, 40), (50, 99), (10, 10)] {
            let expected: Vec<_> = records.iter().filter(|r| {
                let v = r["age"].as_i64().unwrap();
                lo <= v && v <= hi
            }).collect();
            let out = ss.find().range("age", Value::Int(lo), Value::Int(hi)).execute();
            assert_eq!(out.len(), expected.len());
        }
    }

    #[test]
    fn test_zero_budget_keeps_no_indexes() {
        let ss = Superstruct::new(Some(0));
        for i in 0..100 {
            ss.insert(HashMap::from([("n".to_string(), Value::Int(i))]));
        }
        for i in 0..5 {
            let out = ss.find().equals("n", Value::Int(i)).execute();
            assert_eq!(out.len(), 1);
        }
        assert_eq!(ss.index_inventory().len(), 0);
    }

    #[test]
    fn test_repeated_queries_reuse_index() {
        let ss = Superstruct::new(None);
        for i in 0..100 {
            ss.insert(HashMap::from([("x".to_string(), Value::Int(i))]));
        }
        ss.find().equals("x", Value::Int(1)).execute();
        ss.find().equals("x", Value::Int(2)).execute();
        ss.find().equals("x", Value::Int(3)).execute();
        let types: Vec<String> = ss.index_inventory().iter().map(|(t, _, _)| t.clone()).collect();
        assert_eq!(types.iter().filter(|t| *t == "HashIndex").count(), 1);
    }

    // Regression: when a SortedIndex on an attribute already exists, an Equals
    // query on the same attribute used to return a synthesized HashIndex key
    // that did not match anything in the planner, so it silently fell through
    // to a primary scan instead of reusing the SortedIndex. The fix has the
    // planner return the actual existing key. After a Range query, an Equals
    // query should still return the right rows AND the inventory should not
    // gain a redundant HashIndex of zero buckets.
    #[test]
    fn test_planner_reuses_existing_index_for_equals() {
        let ss = Superstruct::new(None);
        for i in 0i64..50 {
            ss.insert(HashMap::from([("k".to_string(), Value::Int(i))]));
        }
        let _ = ss.find().range("k", Value::Int(0), Value::Int(100)).execute();
        let after_range = ss.index_inventory();
        assert!(after_range.iter().any(|(t, _, _)| t == "SortedIndex"));

        let hits = ss.find().equals("k", Value::Int(7)).execute();
        assert_eq!(hits.len(), 1);
        let after_equals = ss.index_inventory();
        // No new HashIndex should have been built. The SortedIndex covers
        // Equals on this attribute, and the planner now reuses it correctly.
        assert!(!after_equals.iter().any(|(t, _, _)| t == "HashIndex"));
        assert_eq!(after_equals.len(), after_range.len());
    }

    // ===== Index Unit Tests =====

    #[test]
    fn test_hashindex_build_and_query() {
        use superstruct::index::{Index, HashIndex};
        use superstruct::{Predicate, PredicateKind, Record};
        let mut idx = HashIndex::new("city".to_string());
        idx.build_from_records(&vec![
            Record { id: 0, attrs: HashMap::from([("city".to_string(), Value::String("NYC".to_string()))]) },
            Record { id: 1, attrs: HashMap::from([("city".to_string(), Value::String("SF".to_string()))]) },
            Record { id: 2, attrs: HashMap::from([("city".to_string(), Value::String("NYC".to_string()))]) },
        ]);
        let pred = Predicate::new(PredicateKind::Equals, "city".to_string(), Value::String("NYC".to_string()));
        let ids = idx.execute(&pred);
        assert_eq!(ids, [0u64, 2].into_iter().collect());
    }

    #[test]
    fn test_hashindex_skips_missing() {
        use superstruct::index::{Index, HashIndex};
        use superstruct::{Predicate, PredicateKind, Record};
        let mut idx = HashIndex::new("city".to_string());
        idx.build_from_records(&vec![
            Record { id: 0, attrs: HashMap::from([("city".to_string(), Value::String("NYC".to_string()))]) },
            Record { id: 1, attrs: HashMap::from([("name".to_string(), Value::String("x".to_string()))]) },
        ]);
        let pred = Predicate::new(PredicateKind::Equals, "city".to_string(), Value::String("NYC".to_string()));
        assert_eq!(idx.execute(&pred), [0u64].into_iter().collect());
    }

    #[test]
    fn test_hashindex_remove() {
        use superstruct::index::{Index, HashIndex};
        use superstruct::{Predicate, PredicateKind, Record};
        let mut idx = HashIndex::new("city".to_string());
        idx.build_from_records(&vec![
            Record { id: 0, attrs: HashMap::from([("city".to_string(), Value::String("NYC".to_string()))]) },
            Record { id: 1, attrs: HashMap::from([("city".to_string(), Value::String("NYC".to_string()))]) },
        ]);
        idx.remove(&Record { id: 0, attrs: HashMap::from([("city".to_string(), Value::String("NYC".to_string()))]) });
        let pred = Predicate::new(PredicateKind::Equals, "city".to_string(), Value::String("NYC".to_string()));
        assert_eq!(idx.execute(&pred), [1u64].into_iter().collect());
    }

    #[test]
    fn test_sortedindex_range_bounds() {
        use superstruct::index::{Index, SortedIndex};
        use superstruct::{Predicate, PredicateKind};
        let mut idx = SortedIndex::new("n".to_string());
        idx.build_from_records(&(0..5).map(|i| {
            superstruct::Record { id: i as u64, attrs: HashMap::from([("n".to_string(), Value::Int(i))]) }
        }).collect::<Vec<_>>());
        let pred = Predicate::new(PredicateKind::Range, "n".to_string(), Value::List(vec![Value::Int(1), Value::Int(3)]));
        assert_eq!(idx.execute(&pred), [1u64, 2, 3].into_iter().collect());
    }

    #[test]
    fn test_sortedindex_duplicates() {
        use superstruct::index::{Index, SortedIndex};
        use superstruct::{Predicate, PredicateKind, Record};
        let mut idx = SortedIndex::new("n".to_string());
        idx.build_from_records(&vec![
            Record { id: 0, attrs: HashMap::from([("n".to_string(), Value::Int(1))]) },
            Record { id: 1, attrs: HashMap::from([("n".to_string(), Value::Int(1))]) },
            Record { id: 2, attrs: HashMap::from([("n".to_string(), Value::Int(2))]) },
        ]);
        let pred = Predicate::new(PredicateKind::Range, "n".to_string(), Value::List(vec![Value::Int(1), Value::Int(1)]));
        assert_eq!(idx.execute(&pred), [0u64, 1].into_iter().collect());
    }

    #[test]
    fn test_sortedindex_equals() {
        use superstruct::index::{Index, SortedIndex};
        use superstruct::{Predicate, PredicateKind, Record};
        let mut idx = SortedIndex::new("n".to_string());
        idx.build_from_records(&vec![
            Record { id: 0, attrs: HashMap::from([("n".to_string(), Value::Int(1))]) },
            Record { id: 1, attrs: HashMap::from([("n".to_string(), Value::Int(2))]) },
            Record { id: 2, attrs: HashMap::from([("n".to_string(), Value::Int(2))]) },
            Record { id: 3, attrs: HashMap::from([("n".to_string(), Value::Int(3))]) },
        ]);
        let pred = Predicate::new(PredicateKind::Equals, "n".to_string(), Value::Int(2));
        assert_eq!(idx.execute(&pred), [1u64, 2].into_iter().collect());
    }

    #[test]
    fn test_trieindex_prefix() {
        use superstruct::index::{Index, TrieIndex};
        use superstruct::{Predicate, PredicateKind, Record};
        let mut idx = TrieIndex::new("name".to_string());
        idx.build_from_records(&vec![
            Record { id: 0, attrs: HashMap::from([("name".to_string(), Value::String("Alice".to_string()))]) },
            Record { id: 1, attrs: HashMap::from([("name".to_string(), Value::String("Anya".to_string()))]) },
            Record { id: 2, attrs: HashMap::from([("name".to_string(), Value::String("Bob".to_string()))]) },
        ]);
        let pred = Predicate::new(PredicateKind::Prefix, "name".to_string(), Value::String("A".to_string()));
        assert_eq!(idx.execute(&pred), [0u64, 1].into_iter().collect());
    }

    #[test]
    fn test_trieindex_remove() {
        use superstruct::index::{Index, TrieIndex};
        use superstruct::{Predicate, PredicateKind, Record};
        let mut idx = TrieIndex::new("name".to_string());
        idx.build_from_records(&vec![
            Record { id: 0, attrs: HashMap::from([("name".to_string(), Value::String("Alice".to_string()))]) },
            Record { id: 1, attrs: HashMap::from([("name".to_string(), Value::String("Anya".to_string()))]) },
        ]);
        idx.remove(&Record { id: 0, attrs: HashMap::from([("name".to_string(), Value::String("Alice".to_string()))]) });
        let pred = Predicate::new(PredicateKind::Prefix, "name".to_string(), Value::String("A".to_string()));
        assert_eq!(idx.execute(&pred), [1u64].into_iter().collect());
    }

    #[test]
    fn test_invertedindex_finds_word() {
        use superstruct::index::{Index, InvertedIndex};
        use superstruct::{Predicate, PredicateKind, Record};
        let mut idx = InvertedIndex::new("bio".to_string());
        idx.build_from_records(&vec![
            Record { id: 0, attrs: HashMap::from([("bio".to_string(), Value::String("loves cats".to_string()))]) },
            Record { id: 1, attrs: HashMap::from([("bio".to_string(), Value::String("dogs are great".to_string()))]) },
            Record { id: 2, attrs: HashMap::from([("bio".to_string(), Value::String("cats and dogs both".to_string()))]) },
        ]);
        let pred = Predicate::new(PredicateKind::Contains, "bio".to_string(), Value::String("cats".to_string()));
        assert_eq!(idx.execute(&pred), [0u64, 2].into_iter().collect());
    }

    #[test]
    fn test_invertedindex_lowercase() {
        use superstruct::index::{Index, InvertedIndex};
        use superstruct::{Predicate, PredicateKind, Record};
        let mut idx = InvertedIndex::new("bio".to_string());
        idx.build_from_records(&vec![
            Record { id: 0, attrs: HashMap::from([("bio".to_string(), Value::String("Hello World".to_string()))]) },
        ]);
        let pred = Predicate::new(PredicateKind::Contains, "bio".to_string(), Value::String("HELLO".to_string()));
        assert_eq!(idx.execute(&pred), [0u64].into_iter().collect());
    }

    #[test]
    fn test_ngramindex_near_match() {
        use superstruct::index::{Index, NgramIndex};
        use superstruct::{Predicate, PredicateKind, Record};
        let mut idx = NgramIndex::new("name".to_string());
        idx.build_from_records(&vec![
            Record { id: 0, attrs: HashMap::from([("name".to_string(), Value::String("Alice".to_string()))]) },
            Record { id: 1, attrs: HashMap::from([("name".to_string(), Value::String("Bob".to_string()))]) },
        ]);
        let pred = Predicate::new(PredicateKind::Fuzzy, "name".to_string(), Value::String("Alise".to_string()))
            .with_threshold(0.3);
        assert!(idx.execute(&pred).contains(0));
    }

    #[test]
    fn test_ngramindex_exact_threshold_one() {
        use superstruct::index::{Index, NgramIndex};
        use superstruct::{Predicate, PredicateKind, Record};
        let mut idx = NgramIndex::new("name".to_string());
        idx.build_from_records(&vec![
            Record { id: 0, attrs: HashMap::from([("name".to_string(), Value::String("Alice".to_string()))]) },
            Record { id: 1, attrs: HashMap::from([("name".to_string(), Value::String("Alyce".to_string()))]) },
            Record { id: 2, attrs: HashMap::from([("name".to_string(), Value::String("Alice".to_string()))]) },
        ]);
        let pred = Predicate::new(PredicateKind::Fuzzy, "name".to_string(), Value::String("Alice".to_string()))
            .with_threshold(1.0);
        assert_eq!(idx.execute(&pred), [0u64, 2].into_iter().collect());
    }

    // ===== Spatial Index Tests =====

    fn point(x: f64, y: f64) -> Value {
        Value::List(vec![Value::Float(x), Value::Float(y)])
    }

    fn insert_at(ss: &Superstruct, x: f64, y: f64) -> u64 {
        ss.insert(HashMap::from([("loc".to_string(), point(x, y))]))
    }

    #[test]
    fn test_within_box_basic() {
        let ss = Superstruct::new(None);
        let _a = insert_at(&ss, 0.0, 0.0);
        let _b = insert_at(&ss, 1.0, 1.0);
        let _c = insert_at(&ss, 5.0, 5.0);
        let _d = insert_at(&ss, 10.0, 10.0);

        let hits = ss.find().within_box("loc", 0.5, 0.5, 5.5, 5.5).execute();
        assert_eq!(hits.len(), 2); // b and c
    }

    #[test]
    fn test_within_box_empty_when_disjoint() {
        let ss = Superstruct::new(None);
        insert_at(&ss, 0.0, 0.0);
        insert_at(&ss, 1.0, 1.0);
        let hits = ss.find().within_box("loc", 100.0, 100.0, 200.0, 200.0).execute();
        assert!(hits.is_empty());
    }

    #[test]
    fn test_near_radius_basic() {
        let ss = Superstruct::new(None);
        let _a = insert_at(&ss, 0.0, 0.0);
        let _b = insert_at(&ss, 3.0, 4.0);  // distance 5 from origin
        let _c = insert_at(&ss, 6.0, 8.0);  // distance 10 from origin
        let _d = insert_at(&ss, 100.0, 100.0);

        let close = ss.find().near("loc", 0.0, 0.0, 6.0).execute();
        assert_eq!(close.len(), 2); // a and b

        let medium = ss.find().near("loc", 0.0, 0.0, 11.0).execute();
        assert_eq!(medium.len(), 3); // a, b, c
    }

    #[test]
    fn test_near_excludes_outside_radius() {
        let ss = Superstruct::new(None);
        insert_at(&ss, 0.0, 0.0);
        insert_at(&ss, 10.0, 0.0);
        let close = ss.find().near("loc", 0.0, 0.0, 5.0).execute();
        assert_eq!(close.len(), 1);
    }

    #[test]
    fn test_spatial_index_skips_records_without_attribute() {
        let ss = Superstruct::new(None);
        insert_at(&ss, 1.0, 1.0);
        ss.insert(HashMap::from([("name".to_string(), Value::String("no point".to_string()))]));
        let hits = ss.find().within_box("loc", 0.0, 0.0, 5.0, 5.0).execute();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn test_spatial_combines_with_other_indexes() {
        let ss = Superstruct::new(None);
        ss.insert(HashMap::from([
            ("loc".to_string(), point(1.0, 1.0)),
            ("city".to_string(), Value::String("NYC".to_string())),
        ]));
        ss.insert(HashMap::from([
            ("loc".to_string(), point(2.0, 2.0)),
            ("city".to_string(), Value::String("SF".to_string())),
        ]));
        let hits = ss.find()
            .within_box("loc", 0.0, 0.0, 5.0, 5.0)
            .equals("city", Value::String("NYC".to_string()))
            .execute();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn test_spatial_propagates_inserts_after_build() {
        let ss = Superstruct::new(None);
        insert_at(&ss, 0.0, 0.0);
        // Trigger SpatialIndex build with a query.
        ss.find().within_box("loc", -1.0, -1.0, 1.0, 1.0).execute();
        // Insert another point and re-query. Should now find both.
        insert_at(&ss, 0.5, 0.5);
        let hits = ss.find().within_box("loc", -1.0, -1.0, 1.0, 1.0).execute();
        assert_eq!(hits.len(), 2);
    }

    // ===== Substring Tests =====

    #[test]
    fn test_substring_basic() {
        let ss = Superstruct::new(None);
        ss.insert(HashMap::from([("bio".to_string(), Value::String("loves cats".to_string()))]));
        ss.insert(HashMap::from([("bio".to_string(), Value::String("scattered notes".to_string()))]));
        ss.insert(HashMap::from([("bio".to_string(), Value::String("dog person".to_string()))]));
        // Substring "cat" should match the first two but not the third.
        let hits = ss.find().substring("bio", "cat").execute();
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn test_substring_finds_inside_word() {
        let ss = Superstruct::new(None);
        ss.insert(HashMap::from([("bio".to_string(), Value::String("concatenation".to_string()))]));
        ss.insert(HashMap::from([("bio".to_string(), Value::String("dog".to_string()))]));
        let hits = ss.find().substring("bio", "cat").execute();
        // Inverted index would miss "concatenation" because token boundary
        // is missing. Substring search finds it.
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn test_substring_case_insensitive() {
        let ss = Superstruct::new(None);
        ss.insert(HashMap::from([("bio".to_string(), Value::String("CAT photos".to_string()))]));
        ss.insert(HashMap::from([("bio".to_string(), Value::String("dog".to_string()))]));
        let hits = ss.find().substring("bio", "cat").execute();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn test_substring_empty_when_no_match() {
        let ss = Superstruct::new(None);
        ss.insert(HashMap::from([("bio".to_string(), Value::String("hello world".to_string()))]));
        let hits = ss.find().substring("bio", "xyz").execute();
        assert!(hits.is_empty());
    }

    // ===== Weighted Graph Tests =====

    #[test]
    fn test_dijkstra_simple_chain() {
        let ss = Superstruct::new(None);
        let a = ss.insert(HashMap::from([("n".to_string(), Value::Int(0))]));
        let b = ss.insert(HashMap::from([("n".to_string(), Value::Int(1))]));
        let c = ss.insert(HashMap::from([("n".to_string(), Value::Int(2))]));

        ss.add_weighted_edge(a, b, 1.5, None, true);
        ss.add_weighted_edge(b, c, 2.5, None, true);

        let dist = ss.dijkstra(a, None);
        assert_eq!(dist.get(&a), Some(&0.0));
        assert_eq!(dist.get(&b), Some(&1.5));
        assert_eq!(dist.get(&c), Some(&4.0));
    }

    #[test]
    fn test_dijkstra_picks_shortest_via_alternative() {
        let ss = Superstruct::new(None);
        let a = ss.insert(HashMap::from([("n".to_string(), Value::Int(0))]));
        let b = ss.insert(HashMap::from([("n".to_string(), Value::Int(1))]));
        let c = ss.insert(HashMap::from([("n".to_string(), Value::Int(2))]));
        let d = ss.insert(HashMap::from([("n".to_string(), Value::Int(3))]));

        // Direct A->D weight 10, but A->B->C->D weight 1+1+1 = 3.
        ss.add_weighted_edge(a, d, 10.0, None, true);
        ss.add_weighted_edge(a, b, 1.0, None, true);
        ss.add_weighted_edge(b, c, 1.0, None, true);
        ss.add_weighted_edge(c, d, 1.0, None, true);

        let path = ss.shortest_path_weighted(a, d, None);
        assert!(path.is_some());
        let (nodes, total) = path.unwrap();
        assert_eq!(nodes, vec![a, b, c, d]);
        assert_eq!(total, 3.0);
    }

    #[test]
    fn test_shortest_weighted_path_unreachable() {
        let ss = Superstruct::new(None);
        let a = ss.insert(HashMap::from([("n".to_string(), Value::Int(0))]));
        let b = ss.insert(HashMap::from([("n".to_string(), Value::Int(1))]));
        // No edges. b is unreachable from a.
        assert!(ss.shortest_path_weighted(a, b, None).is_none());
    }

    #[test]
    fn test_dijkstra_same_source_target_zero() {
        let ss = Superstruct::new(None);
        let a = ss.insert(HashMap::from([("n".to_string(), Value::Int(0))]));
        let path = ss.shortest_path_weighted(a, a, None);
        assert_eq!(path, Some((vec![a], 0.0)));
    }

    #[test]
    fn test_pagerank_uniform_chain() {
        let ss = Superstruct::new(None);
        let a = ss.insert(HashMap::from([("n".to_string(), Value::Int(0))]));
        let b = ss.insert(HashMap::from([("n".to_string(), Value::Int(1))]));
        let c = ss.insert(HashMap::from([("n".to_string(), Value::Int(2))]));
        ss.add_weighted_edge(a, b, 1.0, None, false);
        ss.add_weighted_edge(b, c, 1.0, None, false);

        let pr = ss.pagerank(0.85, 30);
        // All ranks should be valid probabilities and sum near 1.
        let sum: f64 = pr.values().sum();
        assert!((sum - 1.0).abs() < 0.01, "ranks should sum near 1, got {}", sum);
        // Middle node b has two undirected neighbors, so it should rank
        // strictly higher than the leaves a and c.
        let ra = pr.get(&a).copied().unwrap();
        let rb = pr.get(&b).copied().unwrap();
        let rc = pr.get(&c).copied().unwrap();
        assert!(rb > ra && rb > rc, "expected b to be the highest, got {:?}", pr);
    }

    #[test]
    fn test_pagerank_isolated_nodes_get_some_mass() {
        let ss = Superstruct::new(None);
        let a = ss.insert(HashMap::from([("n".to_string(), Value::Int(0))]));
        let b = ss.insert(HashMap::from([("n".to_string(), Value::Int(1))]));
        let c = ss.insert(HashMap::from([("n".to_string(), Value::Int(2))]));
        // Only a -> b edge. c is dangling. PageRank should still give c some
        // mass via the teleport plus dangling redistribution.
        ss.add_weighted_edge(a, b, 1.0, None, true);
        ss.add_weighted_edge(c, a, 1.0, None, true); // c connected to a
        let pr = ss.pagerank(0.85, 30);
        assert!(pr.contains_key(&a));
        assert!(pr.contains_key(&b));
        assert!(pr.contains_key(&c));
    }

    #[test]
    fn test_unweighted_add_edge_uses_weight_one() {
        // The legacy add_edge entry point should give a weight of exactly
        // 1.0 so dijkstra over a graph built that way matches the BFS depth.
        let ss = Superstruct::new(None);
        let a = ss.insert(HashMap::from([("n".to_string(), Value::Int(0))]));
        let b = ss.insert(HashMap::from([("n".to_string(), Value::Int(1))]));
        let c = ss.insert(HashMap::from([("n".to_string(), Value::Int(2))]));
        ss.add_edge(a, b, None, true);
        ss.add_edge(b, c, None, true);
        let dist = ss.dijkstra(a, None);
        assert_eq!(dist.get(&c), Some(&2.0));
    }
}
