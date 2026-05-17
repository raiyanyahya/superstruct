# Contributing

This is a research project built for fun and learning. It is not a production system.

If you want to contribute:

- Open an issue before writing code. This project is intentionally small and focused. Not every addition fits.
- Keep the same style: no Oxford commas, no dashes in comments. The code avoids contractions and possessive apostrophes in prose comments.
- Write a test. Every feature has a corresponding integration test in `tests/integration_test.rs`.
- Run `cargo test` before opening a pull request. All 103 tests must pass.
- Run `cargo fmt` and verify `cargo clippy` produces no warnings.

Pull requests that add entirely new index types are welcome as long as they plug into the existing planner routing with zero API changes to the public facade.

Pull requests that add a SQL parser, a network server, or a storage engine will be politely declined. That is not what this project is.
