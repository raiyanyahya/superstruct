use superstruct::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::Instant;
use rand::Rng;
use rand::rngs::StdRng;
use rand::SeedableRng;

// Heavier benchmark for stress-testing the data structure at one order of
// magnitude above the standard run. Useful for catching memory blowup,
// concurrency contention that only shows up under load, and amortized
// costs that get hidden at small N.

fn section(title: &str) {
    println!("\n{}", "=".repeat(72));
    println!("{}", title);
    println!("{}", "=".repeat(72));
}

fn fmt_us(seconds: f64) -> String {
    format!("{:.2} us", seconds * 1_000_000.0)
}

fn fmt_ms(seconds: f64) -> String {
    format!("{:.1} ms", seconds * 1000.0)
}

fn fmt_s(seconds: f64) -> String {
    format!("{:.2} s", seconds)
}

fn fmt_n(num: usize) -> String {
    let s = num.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn fmt_mb(bytes: usize) -> String {
    format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
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

const NAMES: &[&str] = &[
    "alice", "anya", "andre", "bea", "ben", "cara", "carl", "diana",
    "erin", "fred", "gina", "henry", "ivy", "jack", "kara", "leo",
];

const CITIES: &[&str] = &[
    "NYC", "SF", "LA", "Boston", "Austin", "Seattle", "Denver", "Chicago",
];

const BIOS: &[&str] = &[
    "loves cats and long walks",
    "dog person all the way",
    "cat owner who also walks dogs",
    "runs marathons every weekend",
    "indie game developer and coffee fan",
    "mountain biker and trail runner",
    "into machine learning and old novels",
    "espresso enthusiast with a sourdough hobby",
];

fn populate(n: usize) -> Superstruct {
    let ss = Superstruct::new(None);
    let mut rng = StdRng::seed_from_u64(42);
    for _ in 0..n {
        let mut attrs = HashMap::new();
        attrs.insert("name".to_string(), Value::String(NAMES[rng.gen_range(0..NAMES.len())].to_string()));
        attrs.insert("age".to_string(), Value::Int(rng.gen_range(18..81)));
        attrs.insert("score".to_string(), Value::Int(rng.gen_range(0..101)));
        attrs.insert("city".to_string(), Value::String(CITIES[rng.gen_range(0..CITIES.len())].to_string()));
        attrs.insert("bio".to_string(), Value::String(BIOS[rng.gen_range(0..BIOS.len())].to_string()));
        ss.insert(attrs);
    }
    ss
}

fn bench_insert_throughput() {
    section("1. Insert throughput at scale");
    println!("{:>12} {:>14} {:>14} {:>20}", "records", "total", "per insert", "throughput");
    for n in [100_000usize, 500_000, 1_000_000] {
        let elapsed = time_block(|| { populate(n); });
        let per = elapsed / n as f64;
        let ops_per_sec = (n as f64 / elapsed) as usize;
        println!(
            "{:>12} {:>14} {:>14} {:>15} ops/sec",
            fmt_n(n),
            if elapsed >= 1.0 { fmt_s(elapsed) } else { fmt_ms(elapsed) },
            fmt_us(per),
            fmt_n(ops_per_sec),
        );
    }
}

fn bench_query_latency() {
    section("2. Query latency on a 200k store. 50 warm calls. Returns full result set");
    let ss = populate(200_000);

    struct Q<'a> {
        label: &'a str,
        run: Box<dyn Fn() + 'a>,
    }
    let queries: Vec<Q> = vec![
        Q { label: "equals city = NYC",     run: Box::new(|| { ss.find().equals("city", Value::String("NYC".to_string())).execute(); }) },
        Q { label: "range age 25..35",      run: Box::new(|| { ss.find().range("age", Value::Int(25), Value::Int(35)).execute(); }) },
        Q { label: "prefix name = a",       run: Box::new(|| { ss.find().prefix("name", "a").execute(); }) },
        Q { label: "contains bio cat",      run: Box::new(|| { ss.find().contains("bio", "cat").execute(); }) },
        Q { label: "fuzzy name alise 0.4",  run: Box::new(|| { ss.find().fuzzy("name", "alise", 0.4).execute(); }) },
    ];

    println!("{:>26} {:>14} {:>14} {:>10}", "query", "cold", "warm avg", "speedup");
    println!("  Note: warm time at 200k records is dominated by result hydration");
    println!("  (cloning 25k+ HashMap<String, Value> per query) not index lookup.");
    for q in &queries {
        let cold = time_block(|| { (q.run)(); });
        let warm_avg = time_block_ntimes(50, || { (q.run)(); }) / 50.0;
        let speedup = if warm_avg > 0.0 { cold / warm_avg } else { 0.0 };
        println!(
            "{:>26} {:>14} {:>14} {:>9.0}x",
            q.label, fmt_ms(cold), fmt_us(warm_avg), speedup,
        );
    }

    // Same five query kinds but bounded to top_k 10 so result hydration cost
    // is fixed and what we see is the index lookup itself.
    section("2b. Same queries with top_k 10. Index-only cost");
    let queries_topk: Vec<Q> = vec![
        Q { label: "equals city + topk",    run: Box::new(|| { ss.find().equals("city", Value::String("NYC".to_string())).top_k("score", 10, true).execute(); }) },
        Q { label: "range age + topk",      run: Box::new(|| { ss.find().range("age", Value::Int(25), Value::Int(35)).top_k("score", 10, true).execute(); }) },
        Q { label: "prefix name + topk",    run: Box::new(|| { ss.find().prefix("name", "a").top_k("score", 10, true).execute(); }) },
        Q { label: "contains bio + topk",   run: Box::new(|| { ss.find().contains("bio", "cat").top_k("score", 10, true).execute(); }) },
        Q { label: "fuzzy name + topk",     run: Box::new(|| { ss.find().fuzzy("name", "alise", 0.4).top_k("score", 10, true).execute(); }) },
    ];
    println!("{:>26} {:>14}", "query", "warm avg");
    for q in &queries_topk {
        // Indexes are already warm from section 2, so just measure warm.
        let warm_avg = time_block_ntimes(50, || { (q.run)(); }) / 50.0;
        println!("{:>26} {:>14}", q.label, fmt_us(warm_avg));
    }
}

fn bench_compound_vs_scan() {
    section("3. Compound query at 500k records. 50 runs averaged");

    let ss = Superstruct::new(None);
    let mut rng = StdRng::seed_from_u64(0);
    let mut records: Vec<HashMap<String, Value>> = Vec::with_capacity(500_000);
    for _ in 0..500_000 {
        let mut attrs = HashMap::new();
        attrs.insert("name".to_string(), Value::String(NAMES[rng.gen_range(0..NAMES.len())].to_string()));
        attrs.insert("age".to_string(), Value::Int(rng.gen_range(18..81)));
        attrs.insert("score".to_string(), Value::Int(rng.gen_range(0..101)));
        attrs.insert("city".to_string(), Value::String(CITIES[rng.gen_range(0..CITIES.len())].to_string()));
        attrs.insert("bio".to_string(), Value::String(BIOS[rng.gen_range(0..BIOS.len())].to_string()));
        records.push(attrs.clone());
        ss.insert(attrs);
    }

    // Warm the indexes once each
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

    let runs = 50;
    let compound_avg = time_block_ntimes(runs, compound) / runs as f64;
    let scan_avg = time_block_ntimes(runs, scan) / runs as f64;
    println!("  compound (indexed): {}", fmt_ms(compound_avg));
    println!("  scan (Rust iter):    {}", fmt_ms(scan_avg));
    if compound_avg > 0.0 {
        println!("  speedup:             {:.0}x", scan_avg / compound_avg);
    }
}

fn bench_concurrency() {
    section("4. Concurrency. 16 writers + 16 readers. 5,000 ops each");

    let ss = Arc::new(Superstruct::new(None));
    let n_writers = 16;
    let n_readers = 16;
    let ops_per_thread = 5_000;

    // Pre-warm a HashIndex on city so readers hit the read fast path
    // immediately. Otherwise the first reader has to build the index under
    // the planner write lock and skews the early seconds.
    for _ in 0..1000 {
        let mut attrs = HashMap::new();
        attrs.insert("city".to_string(), Value::String("NYC".to_string()));
        attrs.insert("n".to_string(), Value::Int(0));
        ss.insert(attrs);
    }
    ss.find().equals("city", Value::String("NYC".to_string())).execute();

    let mut handles = Vec::new();
    for seed in 0..n_writers {
        let ss = ss.clone();
        handles.push(thread::spawn(move || {
            let mut rng = StdRng::seed_from_u64(seed as u64);
            for _ in 0..ops_per_thread {
                let mut attrs = HashMap::new();
                attrs.insert("city".to_string(), Value::String(CITIES[rng.gen_range(0..CITIES.len())].to_string()));
                attrs.insert("n".to_string(), Value::Int(rng.gen_range(0..1_000_000)));
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

    println!("  total {} operations in {}", fmt_n(total_ops), fmt_s(elapsed));
    println!("  throughput: {} ops/sec", fmt_n((total_ops as f64 / elapsed) as usize));
    println!("  final record count: {}", fmt_n(ss.len()));
}

fn bench_read_only_concurrency() {
    section("5. Read-only concurrency. 16 readers. 10,000 queries each. top_k 10");

    let ss = Arc::new(populate(200_000));
    // Warm every index ahead of the parallel run. Use top_k so result
    // hydration stays fixed at 10 records per query, isolating lock and
    // index-traversal cost from allocator throughput on huge result sets.
    let warm = || {
        ss.find().equals("city", Value::String("NYC".to_string())).top_k("score", 10, true).execute();
        ss.find().range("age", Value::Int(25), Value::Int(35)).top_k("score", 10, true).execute();
        ss.find().prefix("name", "a").top_k("score", 10, true).execute();
        ss.find().contains("bio", "cat").top_k("score", 10, true).execute();
    };
    warm();

    let n_readers = 16;
    let ops_per_thread = 10_000;
    let mut handles = Vec::new();
    for tid in 0..n_readers {
        let ss = ss.clone();
        handles.push(thread::spawn(move || {
            for i in 0..ops_per_thread {
                match (tid + i) % 4 {
                    0 => { ss.find().equals("city", Value::String("NYC".to_string())).top_k("score", 10, true).execute(); }
                    1 => { ss.find().range("age", Value::Int(25), Value::Int(35)).top_k("score", 10, true).execute(); }
                    2 => { ss.find().prefix("name", "a").top_k("score", 10, true).execute(); }
                    _ => { ss.find().contains("bio", "cat").top_k("score", 10, true).execute(); }
                }
            }
        }));
    }

    let t0 = Instant::now();
    for h in handles {
        h.join().unwrap();
    }
    let elapsed = t0.elapsed().as_secs_f64();
    let total_ops = n_readers * ops_per_thread;
    println!("  total {} read queries in {}", fmt_n(total_ops), fmt_s(elapsed));
    println!("  throughput: {} ops/sec", fmt_n((total_ops as f64 / elapsed) as usize));
    println!("  per-thread average: {} ops/sec", fmt_n((total_ops as f64 / elapsed / n_readers as f64) as usize));
}

fn bench_memory_inventory() {
    section("6. Memory footprint at 200k records");

    let ss = populate(200_000);
    ss.find().equals("city", Value::String("NYC".to_string())).execute();
    ss.find().range("age", Value::Int(25), Value::Int(35)).execute();
    ss.find().prefix("name", "a").execute();
    ss.find().contains("bio", "cat").execute();
    ss.find().fuzzy("name", "alise", 0.4).execute();

    println!("  {:>15} {:>12} {:>14} {:>14}", "type", "attribute", "bytes", "human");
    let mut inventory = ss.index_inventory();
    inventory.sort_by(|a, b| b.2.cmp(&a.2));

    let mut total = 0usize;
    for (kind, attr, size) in &inventory {
        println!("  {:>15} {:>12} {:>14} {:>14}", kind, attr, fmt_n(*size), fmt_mb(*size));
        total += size;
    }
    println!("  {:>15} {:>12} {:>14} {:>14}", "total", "", fmt_n(total), fmt_mb(total));
}

fn main() {
    bench_insert_throughput();
    bench_query_latency();
    bench_compound_vs_scan();
    bench_concurrency();
    bench_read_only_concurrency();
    bench_memory_inventory();
    println!("\nDone.");
}
