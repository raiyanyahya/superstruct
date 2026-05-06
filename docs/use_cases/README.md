# Superstruct use cases

Three real-world integration patterns, with code and context. Each document covers a scenario, where Superstruct fits in the application architecture, complete working code, and why it replaces the usual approach.

1. [E-commerce product search backend](01-ecommerce-product-search.md) -- A web API serving 200k products with category, price range, brand prefix, and full-text search. Superstruct sits inside the request handler behind an Arc, replacing a tangle of single-purpose data structures.

2. [Game server spatial matchmaking](02-game-server-spatial-matchmaking.md) -- A multiplayer server matching 50k concurrent players by 2D proximity, skill band, and region. Superstruct runs in the game loop tick with no external database process.

3. [Internal customer data explorer](03-customer-data-explorer.md) -- A CLI tool on a support engineer's laptop. Loads a weekly data dump, answers fuzzy name searches and compound filters, and precomputes PageRank on the referral graph in a REPL.

## The common pattern

All three cases share the same shape. An application has structured records in memory. It needs multiple access patterns: equality, range, prefix, full-text, fuzzy, spatial, substring, graph traversal. Without Superstruct the developer wires up several single-purpose data structures by hand, keeps them synchronized across inserts and deletes, and hopes the combination is correct. With Superstruct the same records go into one object. The structure discovers what indexes to build based on the queries that actually arrive.

The code lives at the application layer -- inside a request handler, inside a game loop, inside a REPL -- not behind a network boundary. There is no separate process, no connection pool, no client library. The data structure is the database.
