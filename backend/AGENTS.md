# Agent Guidelines

- Never run destructive git commands like `git reset --hard` or `git checkout --` without explicit user approval.
- Keep individual source files around 500 lines or fewer to support readability and review.
- Break larger systems into smaller modules or crates so responsibilities stay focused.
- Prefer configurable parameters over hard-coded values; expose settings through configuration files or environment variables when possible.
- Remove dead or unused code when features are deprecated to keep the codebase lean.
- Keep naming verbose and easy to understand
- Never suggest static plugins as a workaround or solution.

Follow these principles to keep the workspace safe, maintainable, and adaptable.
