# Internal customer data explorer

Scenario. A support team searches a few million customer records by partial name match, email domain, join date range, and plan type. They also want PageRank-style importance scoring based on referral chains. You build a CLI tool that loads a weekly data dump, warms indexes, then drops into an interactive REPL.

## Where it fits in the app

On a support engineer laptop, as a single binary with no external setup. The weekly data dump is a JSON file exported from the data warehouse. The tool loads it, builds whatever indexes the support team queries actually need and precomputes PageRank over the referral graph. The entire state lives in memory. The interface is two commands and a quit.

```rust
use superstruct::{Superstruct, Value};
use std::collections::HashMap;
use std::io::{self, Write};

fn main() {
    let ss = Superstruct::load("customers.json", None).unwrap();

    // Warm the indexes support will actually hit
    ss.find().range("joined", Value::Int(2020), Value::Int(2025)).execute();
    ss.find().prefix("email", "gmail").execute();
    ss.find().substring("name", "smith").execute();
    ss.find().fuzzy("name", "johnson", 0.5).execute();
    ss.find().equals("plan", Value::String("enterprise".into())).execute();

    // Precompute referral network PageRank
    let importance = ss.pagerank(0.85, 30);

    println!("Loaded {} customers. {} indexes warm. Type a query or 'help'.",
             ss.len(), ss.index_inventory().len());

    loop {
        print!("> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let args: Vec<&str> = input.trim().split_whitespace().collect();

        match args.first().copied() {
            Some("fuzzy") if args.len() >= 3 => {
                let results = ss.find()
                    .fuzzy("name", args[1], args[2].parse().unwrap_or(0.5))
                    .top_k("joined", 10, true)
                    .execute();
                for r in &results {
                    let id = r.get("_id").and_then(|v| v.as_i64()).unwrap_or(0);
                    let name = r.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                    let rank = importance.get(&(id as u64)).copied().unwrap_or(0.0);
                    println!("  {name} (PageRank: {rank:.4})");
                }
            }
            Some("compound") => {
                let results = ss.find()
                    .range("joined", Value::Int(2022), Value::Int(2024))
                    .equals("plan", Value::String("enterprise".into()))
                    .prefix("email", "admin")
                    .execute();
                println!("  {} matches", results.len());
            }
            Some("quit") => break,
            _ => println!("  fuzzy <name> <threshold>  |  compound  |  quit"),
        }
    }
}
```

## Why this instead of the usual approach

The standard answer here is to load the dump into SQLite and write SQL by hand, or build a small web dashboard with a filter form. Both take time to build and have a user interface learning curve.

This tool is one binary compiled from one file, distributed by dropping it on the support engineer machine. It has no setup step beyond pointing at the latest dump file. The fingerprint search uses trigram fuzzy matching, which SQLite cannot do without an extension. The PageRank over the referral graph answers a question about customer importance that the support team could not previously ask. The query language is two commands and a quit. It fits on a single screen.
