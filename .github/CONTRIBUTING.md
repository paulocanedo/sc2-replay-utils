# Contributing

Thanks for your interest in contributing to sc2-replay-utils!

## Getting Started

1. **Fork and clone** the repository
2. Install Rust 1.85+ (edition 2024)
3. On Linux, install GTK3 dev libraries: `sudo apt-get install -y libgtk-3-dev`
4. Build: `cargo build --release`
5. Run tests: `cargo test --release`

## Development Workflow

1. Create a feature branch from `master`
2. Make your changes
3. Ensure tests pass: `cargo test --release`
4. Commit using [Conventional Commits](https://www.conventionalcommits.org/):
   - `feat(scope): add new feature`
   - `fix(scope): fix the bug`
   - `refactor(scope): restructure code`
5. Open a pull request against `master`

### Common Scopes

`build_order`, `timeline`, `charts`, `replay`, `gui`, `library`, `map`, `balance`, `army-value`, `supply-block`

## Project Structure

See [CLAUDE.md](../CLAUDE.md) for a detailed architectural overview.

## Golden Tests

If your changes affect build order parsing, the golden CSV tests may fail. To update them:

```sh
cargo test --bin sc2-replay-utils bless_build_order_goldens -- --ignored
```

Review the diff in `examples/golden/` to confirm the changes are expected before committing.

## Code Style

- Follow standard Rust formatting (`cargo fmt`)
- Keep modules focused — domain logic in `src/`, GUI code in `src/gui/`
- Inline tests with `#[cfg(test)]` in the same file
- Comments and documentation in English
