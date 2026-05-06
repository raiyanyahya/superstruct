# Game server spatial matchmaking

Scenario. A multiplayer game server tracks 50k concurrent players on a 2D world map. Matchmaking needs to find players within a radius, filtered by skill tier and map region. The old code called PostGIS with a network round-trip. The new code puts Superstruct in the game server process.

## Where it fits in the app

Inside the game server loop, on the matchmaking tick hot path. Players insert on connection, query for opponents every few seconds, and delete on disconnect. The spatial index builds automatically on the first `near()` call and stays hot. Combining `near + range` uses roaring bitmap intersection, so with 50k players the matchmaking tick runs in microseconds.

```rust
use superstruct::{Superstruct, Value};
use std::collections::HashMap;
use std::sync::Arc;

struct GameServer {
    players: Arc<Superstruct>,
}

impl GameServer {
    fn add_player(&self, x: f64, y: f64, skill: i64, region: &str) -> u64 {
        self.players.insert(HashMap::from([
            ("pos".into(),   Value::List(vec![Value::Float(x), Value::Float(y)])),
            ("skill".into(), Value::Int(skill)),
            ("region".into(), Value::String(region.into())),
        ]))
    }

    fn find_nearby_opponents(
        &self, my_id: u64, x: f64, y: f64, skill_range: i64, radius: f64,
    ) -> Vec<HashMap<String, Value>> {
        let my_skill = self.players.get(my_id)
            .and_then(|r| r.get("skill").and_then(|v| v.as_i64()))
            .unwrap_or(0);

        self.players.find()
            .near("pos", x, y, radius)
            .range("skill",
                Value::Int(my_skill - skill_range),
                Value::Int(my_skill + skill_range),
            )
            .top_k("skill", 5, true)
            .execute()
    }

    fn cleanup_offline(&self, player_id: u64) {
        self.players.delete(player_id);
    }
}
```

## Why this instead of the usual approach

The standard way to do spatial matching is an external database. PostGIS, Redis GEO commands, or a quadtree library wired into the game loop by hand. Each approach adds a network boundary or a separate data structure you must synchronize.

Superstruct holds the spatial index inside the same object as the skill hash index and the region hash index. One insert places the player into all three. One delete removes them from all three. No external process. No network hop between the game loop and the data. No schema migration when a new attribute gets added next sprint. The game server binary ships with zero database dependencies.
