# E-commerce product search backend

Scenario. You run an online store with ~200k products. Users filter by category, price range, brand name prefix, and free-text search on descriptions. The frontend sends query parameters. Your Rust API server runs Superstruct in-process.

## Where it fits in the app

Inside your API handler, in the request/response hot path. The store sits behind an `Arc<Superstruct>` shared across all handler threads. At startup you load a JSON snapshot and warm the indexes your users hit most often. Every request takes a read lock with no coordination between concurrent threads.

```rust
use superstruct::{Superstruct, Value};
use std::collections::HashMap;
use std::sync::Arc;
use actix_web::{web, App, HttpServer, HttpResponse};

struct AppState {
    store: Arc<Superstruct>,
}

async fn search_products(
    state: web::Data<AppState>,
    params: web::Query<ProductSearchParams>,
) -> HttpResponse {
    let mut q = state.store.find();

    if let Some(ref category) = params.category {
        q = q.equals("category", Value::String(category.clone()));
    }
    if let (Some(lo), Some(hi)) = (params.price_min, params.price_max) {
        q = q.range("price", Value::Float(lo), Value::Float(hi));
    }
    if let Some(ref brand_prefix) = params.brand {
        q = q.prefix("brand", brand_prefix);
    }
    if let Some(ref keyword) = params.keyword {
        q = q.substring("description", keyword);
    }

    let results = q.top_k("popularity", 20, true).execute();
    HttpResponse::Ok().json(results)
}

// Startup: load snapshot, warm indexes
#[actix_web::main]
async fn main() {
    let ss = Superstruct::load("products.json", None).unwrap();

    // Warm common query patterns so first user request is instant
    ss.find().range("price", Value::Float(0.0), Value::Float(5000.0)).execute();
    ss.find().prefix("brand", "S").execute();
    ss.find().substring("description", "wireless").execute();

    let state = web::Data::new(AppState { store: Arc::new(ss) });

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .route("/search", web::get().to(search_products))
    })
    .bind("0.0.0.0:8080").unwrap()
    .run().await.unwrap();
}
```

## Why this instead of the usual approach

The standard alternative is a Vec of product structs filtered by iterator chains. Then someone adds a Redis sorted set for price ranges. Then someone adds a separate trie crate for brand prefixes. Then someone adds an inverted index library for descriptions. Now you have five data structures to keep in sync on every product insert, update, and delete. Half of them carry the same data in different shapes.

Superstruct replaces all of them. The 200k products sit in about 20 MB of memory with roaring bitmap posting lists. Five filter types route to five indexes automatically on first query. The compound query `range + prefix + substring` runs in under a millisecond. No index was declared. No schema was written.
