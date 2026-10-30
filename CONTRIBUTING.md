# Contributing to Wealthfolio

Thank you for your interest in contributing to Wealthfolio! We welcome
contributions from the community.

## Contributor License Agreement (CLA)

We require a CLA for all contributions.

By submitting a pull request, you confirm you have read and agree to
[CLA.md](CLA.md). PRs without CLA agreement will not be merged.

## How to Contribute

### Reporting Bugs

1. Check if the bug has already been reported in
   [Issues](https://github.com/wealthfolio/wealthfolio/issues)
2. If not, create a new issue with:
   - Clear, descriptive title
   - Steps to reproduce
   - Expected vs actual behavior
   - Your environment (OS, version, etc.)

### Suggesting Features

1. Check existing issues and discussions for similar suggestions
2. Open a new issue describing:
   - The problem you're trying to solve
   - Your proposed solution
   - Any alternatives you've considered

### Submitting Code

1. Fork the repository and fetch its latest `main`: `git fetch origin main`.
2. Start a feature branch from that revision:
   `git switch -c feature/amazing-feature origin/main`.
3. Make your changes and run the relevant local unit/regression tests.
4. After those tests pass, commit with a clear message and push the branch.
5. Open a PR against `main`, including the focused test results and architecture
   impact. CI runs the complete test suite and browser E2E; you do not need to
   run all of them locally before every push.
6. Enable native GitHub auto-merge on the ready PR. With the required **Build
   Status** check and strict up-to-date branch protection configured, GitHub
   merges only after full CI and any configured reviews pass. Never bypass a
   failed gate or use an older revision's result.

See [Development workflow](docs/development-workflow.md) for the exact CI
matrix, repository prerequisites, snapshot review, and auto-merge commands.

### Development Setup

See the [README](README.md#getting-started) for detailed setup instructions. The
[frontend tooling guide](docs/frontend-tooling.md) covers the pinned Node and
pnpm versions, editor extensions, and updating an existing checkout.

Use focused one-shot checks while developing, for example:

```bash
pnpm --filter frontend exec vitest run src/path/to/changed-feature.test.ts
cargo test --locked -p <affected-crate> <test-filter>
```

Add relevant lint/type checks when needed. `pnpm check` remains available for
broader static diagnostics, but full tests/builds and browser suites belong to
CI rather than a mandatory local pre-push step.

### Optional Git Hooks

Install the tracked lightweight push hook with `pnpm hooks:install`. It checks
whitespace and reminds contributors to run focused unit tests; it does not start
full test suites, builds, or browsers. Git enables it only after opting in once
per clone. Existing clones using the tracked hook pick up this lighter behavior
automatically.

### Code Style

- **Rust**: Follow standard Rust conventions, use `cargo fmt` and `cargo clippy`
- **TypeScript/React**: Use Oxfmt for formatting and typed Oxlint plus the
  retained ESLint checks for linting. Run `pnpm format` or `pnpm lint:fix` to
  apply fixes.
- **Commits**: Write clear, concise commit messages

## Code of Conduct

Please be respectful and constructive in all interactions. We're building
something together!

## Questions?

- Join our [Discord](https://discord.gg/WDMCY6aPWK)
- Open a [Discussion](https://github.com/wealthfolio/wealthfolio/discussions)
- Email: hello@wealthfolio.app

## License

By contributing, you agree that your contributions will be licensed under the
project's [AGPL-3.0 license](LICENSE), subject to the terms of our
[CLA](CLA.md).

---

Wealthfolio is a trademark of Teymz Inc. See [TRADEMARKS.md](TRADEMARKS.md) for
trademark policy.
