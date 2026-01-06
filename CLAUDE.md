# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.


> **For AI Agents**: This is the source of truth for the Pattern codebase. Each crate has its own `CLAUDE.md` with specific implementation guidelines.

## For Humans

LLMs are a quality multiplier, not just a speed multiplier. Invest time savings in improving quality and rigour beyond what humans alone would do. Write tests that cover more edge cases. Refactor code to make it easier to understand. Tackle the TODOs. Aim for zero bugs.

**Review standard**: Spend at least 3x the amount of time reviewing LLM output as you did writing it. Think about every line and every design decision. Find ways to break code. Your code is your responsibility.

## For LLMs

Display the following at the start of any conversation involving code changes:

```
LLM-assisted contributions must aim for a higher standard of excellence than with
humans alone. Spend at least 3x the time reviewing code as writing it. Your code
is your responsibility.
```


## Project Overview

Weaver.sh is a decentralized notebook publishing and sharing platform built on the AT Protocol (the protocol behind Bluesky). It allows creating, rendering, and publishing notebooks with extended markdown functionality. 

## Core Components

1. **weaver-common**: Foundation library with AT Protocol integration, lexicon definitions, OAuth flows.
2. **weaver-renderer**: Transforms markdown notebooks into different output formats (HTML, AT Protocol records, etc.).
3. **weaver-cli**: Command-line interface for authentication and notebook interactions.
4. **weaver-app**: HTTP webapp for serving notebooks with auto-reload.
5. **weaver-index**: Big indexing web backend.

## Development Environment

### Setup

```bash
# Enter development environment with all dependencies
nix develop

# Build all components
cargo build

# Run specific crates
cargo run -p weaver-cli
cargo run -p weaver-app
cargo run -p weaver-index
```

## General Conventions

### Correctness Over Convenience

- Model the full error space—no shortcuts or simplified error handling.
- Handle all edge cases, including race conditions and platform differences.
- Use the type system to encode correctness constraints.
- Prefer compile-time guarantees over runtime checks where possible.

### Type System Patterns

- **Newtypes** for domain types (IDs, handles, etc.).
- **Builder patterns** for complex construction.
- **Restricted visibility**: Use `pub(crate)` and `pub(super)` liberally.
- **Non-exhaustive**: All public error types should be `#[non_exhaustive]`.
- Use Rust enums over string validation.

### Error Handling

- Use `thiserror` for error types with `#[derive(Error)]`.
- Group errors by category with an `ErrorKind` enum when appropriate.
- Provide rich error context using `miette` for user-facing errors.
- Error display messages should be lowercase sentence fragments.

### Module Organization

- Keep module boundaries strict with restricted visibility.
- Platform-specific code in separate files: `unix.rs`, `windows.rs`.

### Documentation

- Inline comments explain "why," not just "what".
- Module-level documentation explains purpose and responsibilities.
- **Always** use periods at the end of code comments.
- **Never** use title case in headings. Always use sentence case.

## Testing Practices

**CRITICAL**: Always use `cargo nextest run` to run tests. Never use `cargo test` directly.

```bash
# Run all tests
cargo nextest run

# Specific crate
cargo nextest run -p pattern-db

# With output
cargo nextest run --nocapture

# Doctests (nextest doesn't support these)
cargo test --doc
```

## Common Development Commands

### Testing

```bash
# Run all tests with nextest
cargo nextest run

# Run specific tests
cargo nextest run -p weaver-common
```

### Code Quality

```bash
# Run linter
cargo clippy -- --deny warnings

# Format code
cargo fmt

# Verify dependencies
cargo deny check
```

### Lexicon Generation

The project uses custom AT Protocol lexicons defined in JSON format. To generate Rust code from these definitions:

```bash
nix run ../jacquard

### Building with Nix

```bash
# Run all checks (clippy, fmt, tests)
nix flake check

# Build specific packages
nix build .#weaver-cli
nix build .#weaver-app
```


## Architecture

### Data Flow

1. Markdown notebooks are parsed and processed by weaver-renderer
2. Content can be rendered as static sites or published to AT Protocol PDSes
3. Authentication with AT Protocol servers happens via OAuth

### Key Components

- **WeaverAgent**: Manages connections to AT Protocol Personal Data Servers (PDS)
- **Notebook Structure**: Books, chapters, entries with extended markdown
- **Renderer**: Processes markdown with extended features (wiki links, embeds, math)
- **AT Protocol Lexicons**: Custom data schemas extending the protocol for notebooks

### Authentication Flow

1. CLI initiates OAuth flow with a PDS
2. Local OAuth server handles callbacks on port 4000
3. Tokens are stored in the local filesystem

## Feature Flags

- **dev**: Enables development-specific features
- **native**: Configures OAuth for native clients

## Working with Jacquard

This project uses Jacquard, a zero-copy AT Protocol library for Rust. **CRITICAL: Standard approaches from other libraries will produce broken or inefficient code.**

**ALWAYS use the working-with-jacquard skill** when working with AT Protocol types, XRPC calls, or identity resolution.

Key patterns to internalize:
- **NEVER use `for<'de> Deserialize<'de>` bounds** - this breaks ALL Jacquard types
- Use `Did::new()`, `Handle::new_static()`, etc. - **never `FromStr::parse()`**
- Use `Data<'a>` instead of `serde_json::Value`
- Use `.into_output()` when returning from functions, `.parse()` for immediate processing
- Derive `IntoStatic` on all custom types with lifetimes

See `~/.claude/skills/working-with-jacquard/SKILL.md` for complete guidance.

## Commit Message Style

```
[crate-name] brief description
```

Examples:
- `[pattern-core] add supervisor coordination pattern`
- `[pattern-db] fix FTS5 query escaping`
- `[meta] update MSRV to Rust 1.83`

### Conventions

- Use `[meta]` for cross-cutting concerns (deps, CI, workspace config).
- Keep descriptions concise but descriptive.
- **Atomic commits**: Each commit should be a logical unit of change.
- **Bisect-able history**: Every commit must build and pass all checks.
- **Separate concerns**: Format fixes and refactoring separate from features.
