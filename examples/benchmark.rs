use superstruct::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::Instant;
use rand::Rng;
use rand::rngs::StdRng;
use rand::SeedableRng;

fn section(title: &str) {
    println!("\n{}", "=".repeat(68));
    println!("{}", title);
    println!("{}", "=".repeat(68));
}

fn fmt_us(seconds: f64) -> String {
    format!("{:.2} us", seconds * 1_000_000.0)
}

fn fmt_ms(seconds: f64) -> String {
    format!("{:.3} ms", seconds * 1000.0)
}

fn fmt_k(num: usize) -> String {
    num.to_string()
}

fn time_block(f: impl FnOnce()) -> f64 {
    let t0 = Instant::now();
    f();
    t0.elapsed().as_secs_f64()
}

fn time_block_ntimes(n: usize, mut f: impl FnMut()) -> f64 {
    let t0 = Instant::now();
    for _ in 0..n {
        f();
    }
    t0.elapsed().as_secs_f64()
}

fn populate(n: usize) -> Superstruct {
    let ss = Superstruct::new(None);
    let cities = vec!["NYC", "SF", "LA", "Boston", "Austin"];
    let names = vec![
        "alice", "anya", "andre", "bea", "ben",
        "cara", "carl", "diana", "erin", "fred",
    ];
    let bios = vec![
        "loves cats and long walks",
        "dog person all the way",
        "cat owner who also walks dogs",
        "runs marathons every weekend",
        "indie game developer and coffee fan",
    ];
    let mut rng = StdRng::seed_from_u64(0);

    for _ in 0..n {
        let mut attrs = HashMap::new();
        attrs.insert("name".to_string(), Value::String(names[rng.gen_range(0..names.len())].to_string()));
        attrs.insert("age".to_string(), Value::Int(rng.gen_range(18..81)));
        attrs.insert("score".to_string(), Value::Int(rng.gen_range(0..101)));
        attrs.insert("city".to_string(), Value::String(cities[rng.gen_range(0..cities.len())].to_string()));
        attrs.insert("bio".to_string(), Value::String(bios[rng.gen_range(0..bios.len())].to_string()));
        ss.insert(attrs);
    }
    ss
}

fn bench_insert_throughput() {
    section("1. Insert throughput");
    println!("{:>10} {:>14} {:>14}", "records", "total", "per insert");
    for n in [1_000, 10_000, 50_000] {
        let elapsed = time_block(|| { populate(n); });
        let per = elapsed / n as f64;
        println!("{:>10} {:>14} {:>14} ({:>8} ops/sec)",
            fmt_k(n), fmt_ms(elapsed), fmt_us(per),
            fmt_k((n as f64 / elapsed) as usize),
        );
    }
}

fn bench_query_latency() {
    section("2. Query latency. Cold first call versus warm reuse");
    let ss = populate(20_000);

    struct Query<'a> {
        label: &'a str,
        run: Box<dyn Fn() + 'a>,
    }

    let queries: Vec<Query> = vec![
        Query { label: "equals city = NYC", run: Box::new(|| { ss.find().equals("city", Value::String("NYC".to_string())).execute(); }) },
        Query { label: "range age 25..35",  run: Box::new(|| { ss.find().range("age", Value::Int(25), Value::Int(35)).execute(); }) },
        Query { label: "prefix name = a",   run: Box::new(|| { ss.find().prefix("name", "a").execute(); }) },
        Query { label: "contains bio cat",  run: Box::new(|| { ss.find().contains("bio", "cat").execute(); }) },
        Query { label: "fuzzy name alise",  run: Box::new(|| { ss.find().fuzzy("name", "alise", 0.4).execute(); }) },
    ];

    println!("{:>22} {:>14} {:>14} {:>10}", "query", "cold", "warm avg", "speedup");
    for q in &queries {
        let cold = time_block(|| { (q.run)(); });
        let warm_avg = time_block_ntimes(50, || { (q.run)(); }) / 50.0;
        let speedup = if warm_avg > 0.0 { cold / warm_avg } else { 0.0 };
        println!(
            "{:>22} {:>14} {:>14} {:>9.0}x",
            q.label, fmt_ms(cold), fmt_us(warm_avg), speedup,
        );
    }
}

fn bench_compound_vs_scan() {
    section("3. Compound query. Indexed versus scan");

    let ss = Superstruct::new(None);
    let cities = vec!["NYC", "SF", "LA", "Boston", "Austin"];
    let names = vec![
        "alice", "anya", "andre", "bea", "ben",
        "cara", "carl", "diana", "erin", "fred",
    ];
    let bios = vec![
        "loves cats and long walks",
        "dog person all the way",
        "cat owner who also walks dogs",
        "runs marathons every weekend",
        "indie game developer and coffee fan",
    ];
    let mut rng = StdRng::seed_from_u64(0);

    let mut records: Vec<HashMap<String, Value>> = Vec::new();
    for _ in 0..50_000 {
        let mut attrs = HashMap::new();
        attrs.insert("name".to_string(), Value::String(names[rng.gen_range(0..names.len())].to_string()));
        attrs.insert("age".to_string(), Value::Int(rng.gen_range(18..81)));
        attrs.insert("score".to_string(), Value::Int(rng.gen_range(0..101)));
        attrs.insert("city".to_string(), Value::String(cities[rng.gen_range(0..cities.len())].to_string()));
        attrs.insert("bio".to_string(), Value::String(bios[rng.gen_range(0..bios.len())].to_string()));
        records.push(attrs.clone());
        ss.insert(attrs);
    }

    for _ in 0..2 {
        ss.find().range("age", Value::Int(25), Value::Int(35)).execute();
        ss.find().prefix("name", "a").execute();
        ss.find().equals("city", Value::String("SF".to_string())).execute();
    }

    let compound = || {
        let _ = ss.find()
            .range("age", Value::Int(25), Value::Int(35))
            .prefix("name", "a")
            .equals("city", Value::String("SF".to_string()))
            .execute();
    };

    let scan = || {
        let _: Vec<_> = records.iter().filter(|attrs| {
            let age = attrs.get("age").and_then(|v| v.as_i64()).unwrap_or(-1);
            if !(25..=35).contains(&age) { return false; }
            match attrs.get("name") {
                Some(Value::String(s)) if s.starts_with('a') => {},
                _ => return false,
            }
            attrs.get("city") == Some(&Value::String("SF".to_string()))
        }).collect::<Vec<_>>();
    };

    let runs = 5;
    let compound_avg = time_block_ntimes(runs, compound) / runs as f64;
    let scan_avg = time_block_ntimes(runs, scan) / runs as f64;
    println!("  compound (indexed): {}", fmt_ms(compound_avg));
    println!("  scan (Rust iter):    {}", fmt_ms(scan_avg));
    if compound_avg > 0.0 {
        println!("  speedup:             {:.0}x", scan_avg / compound_avg);
    }
}

fn bench_concurrency() {
    section("4. Concurrency. Mixed readers and writers");

    let ss = Arc::new(Superstruct::new(None));
    let n_writers = 4;
    let n_readers = 4;
    let ops_per_thread = 2_000;

    let mut handles = Vec::new();

    for seed in 0..n_writers {
        let ss = ss.clone();
        handles.push(thread::spawn(move || {
            let mut rng = StdRng::seed_from_u64(seed as u64);
            for _ in 0..ops_per_thread {
                let mut attrs = HashMap::new();
                attrs.insert("city".to_string(), Value::String("NYC".to_string()));
                attrs.insert("n".to_string(), Value::Int(rng.gen_range(0..1000)));
                ss.insert(attrs);
            }
        }));
    }

    for _ in 0..n_readers {
        let ss = ss.clone();
        handles.push(thread::spawn(move || {
            for _ in 0..ops_per_thread {
                let _ = ss.find().equals("city", Value::String("NYC".to_string())).execute();
            }
        }));
    }

    let t0 = Instant::now();
    for h in handles {
        h.join().unwrap();
    }
    let elapsed = t0.elapsed().as_secs_f64();
    let total_ops = (n_writers + n_readers) * ops_per_thread;

    println!("  {} writers + {} readers, {} ops each",
             n_writers, n_readers, fmt_k(ops_per_thread));
    println!("  total {} operations in {}", fmt_k(total_ops), fmt_ms(elapsed));
    println!("  throughput: {} ops/sec", fmt_k((total_ops as f64 / elapsed) as usize));
    println!("  final record count: {}", fmt_k(ss.len()));
}

fn bench_memory_inventory() {
    section("5. Memory footprint of materialized indexes");

    let ss = populate(20_000);
    ss.find().equals("city", Value::String("NYC".to_string())).execute();
    ss.find().range("age", Value::Int(25), Value::Int(35)).execute();
    ss.find().prefix("name", "a").execute();
    ss.find().contains("bio", "cat").execute();
    ss.find().fuzzy("name", "alise", 0.4).execute();

    println!("  {:>15} {:>12} {:>14}", "type", "attribute", "bytes");
    let mut inventory = ss.index_inventory();
    inventory.sort_by(|a, b| b.2.cmp(&a.2));

    let mut total = 0usize;
    for (kind, attr, size) in &inventory {
        println!("  {:>15} {:>12} {:>14}", kind, attr, fmt_k(*size));
        total += size;
    }
    println!("  {:>15} {:>12} {:>14}", "total", "", fmt_k(total));
}

fn bench_spatial() {
    section("6. Spatial queries. 50,000 random points in a 1000x1000 plane");

    let ss = Superstruct::new(None);
    let mut rng = StdRng::seed_from_u64(7);
    for _ in 0..50_000 {
        let mut attrs = HashMap::new();
        attrs.insert("loc".to_string(), Value::List(vec![
            Value::Float(rng.gen_range(0.0..1000.0)),
            Value::Float(rng.gen_range(0.0..1000.0)),
        ]));
        ss.insert(attrs);
    }

    let bbox = || {
        ss.find().within_box("loc", 250.0, 250.0, 750.0, 750.0).execute();
    };
    let near = || {
        ss.find().near("loc", 500.0, 500.0, 50.0).execute();
    };

    println!("{:>22} {:>14} {:>14}", "query", "cold", "warm avg");
    let cold_bbox = time_block(bbox);
    let warm_bbox = time_block_ntimes(50, bbox) / 50.0;
    println!("{:>22} {:>14} {:>14}", "within_box 500x500", fmt_ms(cold_bbox), fmt_us(warm_bbox));
    let cold_near = time_block(near);
    let warm_near = time_block_ntimes(50, near) / 50.0;
    println!("{:>22} {:>14} {:>14}", "near radius 50", fmt_ms(cold_near), fmt_us(warm_near));
}

fn bench_substring() {
    section("7. Substring search vs Contains vs scan. 50,000 records");

    let ss = Superstruct::new(None);
    let mut rng = StdRng::seed_from_u64(13);
    let words = vec![
        "configuration", "concatenate", "scattered notes", "encyclopedia",
        "platypus", "transmission", "concentration", "category",
        "metaphor", "kaleidoscope", "harmonious", "tessellation",
    ];
    let mut records: Vec<HashMap<String, Value>> = Vec::with_capacity(50_000);
    for _ in 0..50_000 {
        let mut attrs = HashMap::new();
        attrs.insert("bio".to_string(), Value::String(words[rng.gen_range(0..words.len())].to_string()));
        ss.insert(attrs.clone());
        records.push(attrs);
    }

    // contains() splits on word boundary so it cannot match "cat" inside
    // "concatenate". substring() can. Both are warmed before timing.
    ss.find().contains("bio", "cat").execute();
    ss.find().substring("bio", "cat").execute();

    let warm_contains = time_block_ntimes(50, || {
        ss.find().contains("bio", "cat").execute();
    }) / 50.0;
    let warm_substring = time_block_ntimes(50, || {
        ss.find().substring("bio", "cat").execute();
    }) / 50.0;
    let scan = time_block_ntimes(50, || {
        let _: Vec<_> = records
            .iter()
            .filter(|r| match r.get("bio") {
                Some(Value::String(s)) => s.to_lowercase().contains("cat"),
                _ => false,
            })
            .collect();
    }) / 50.0;

    let n_contains = ss.find().contains("bio", "cat").execute().len();
    let n_substring = ss.find().substring("bio", "cat").execute().len();

    println!("  contains  (word boundary): {} matches in {}", fmt_k(n_contains), fmt_us(warm_contains));
    println!("  substring (anywhere):      {} matches in {}", fmt_k(n_substring), fmt_us(warm_substring));
    println!("  Rust scan reference:       {} matches in {}", fmt_k(n_substring), fmt_us(scan));
    if warm_substring > 0.0 {
        println!("  substring vs scan: {:.0}x faster", scan / warm_substring);
    }
}

fn bench_graph_algorithms() {
    section("8. Weighted graph. 5,000 nodes, 25,000 edges, Dijkstra + PageRank");

    let ss = Superstruct::new(None);
    let n_nodes = 5_000;
    let mut node_ids: Vec<u64> = Vec::with_capacity(n_nodes);
    for i in 0..n_nodes {
        let id = ss.insert(HashMap::from([("n".to_string(), Value::Int(i as i64))]));
        node_ids.push(id);
    }

    let mut rng = StdRng::seed_from_u64(99);
    let n_edges = 25_000;
    for _ in 0..n_edges {
        let a = node_ids[rng.gen_range(0..n_nodes)];
        let b = node_ids[rng.gen_range(0..n_nodes)];
        if a == b { continue; }
        let weight = rng.gen_range(1.0..10.0);
        ss.add_weighted_edge(a, b, weight, None, true);
    }

    let source = node_ids[0];
    let target = node_ids[n_nodes - 1];

    let dijkstra_time = time_block_ntimes(5, || {
        let _ = ss.dijkstra(source, None);
    }) / 5.0;

    let path_time = time_block_ntimes(5, || {
        let _ = ss.shortest_path_weighted(source, target, None);
    }) / 5.0;

    let pagerank_time = time_block_ntimes(3, || {
        let _ = ss.pagerank(0.85, 30);
    }) / 3.0;

    let dist = ss.dijkstra(source, None);
    println!("  Dijkstra single-source:   {} reachable, {} per call", fmt_k(dist.len()), fmt_ms(dijkstra_time));
    println!("  Shortest path src to tgt: {} per call", fmt_ms(path_time));
    println!("  PageRank 30 iterations:   {} per call", fmt_ms(pagerank_time));
}

fn main() {
    bench_insert_throughput();
    bench_query_latency();
    bench_compound_vs_scan();
    bench_concurrency();
    bench_memory_inventory();
    bench_spatial();
    bench_substring();
    bench_graph_algorithms();
    println!("\nDone.");
}
