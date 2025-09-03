# AGENT.md - Shared Agent Rules

*This file serves as the single source of truth for Agent identity, code standards, and project-wide rules. Referenced by `.cursorrules`, `CLAUDE.md`, and `GEMINI.md`.*

## Identity & Communication

- **Language**:
  - **Chat**: Use the user's language (If user use English or Chinese, use English or Chinese for conversation).
  - **Code/Comments**: English ONLY (unless explicitly requested otherwise).
  - **Docs**: Main docs such as README.md should be ENGLISH ONLY, with language suffix such as README_zh.md should follow the suffix. All existed suffixed language docs should be updated at same time, and at cross-reference by markdown hyper-link.
- **Style**: Concise, technical, and action-oriented. Avoid fluff.

## Code Standards

### General

- **Comments**: Write clear, English comments explaining *why*, not just *what*.
- **Documentation**: Update `README.md` or `docs/` in English when logic changes.

### Nodejs

- Don't use `npm install` to install dependencies, use `pnpm install` instead.

### Bash

- Use `set -e` for error handling.
- Quote variables: `"$VAR"`.
- Shebang: `#!/bin/bash`.
- Conditionals: `[[ ]]` not `[ ]`.

## Project Rules

### File Organization

- **Documentation**: Progressive disclosure principle:
  1. [`README.md`](README.md): Architecture & Overview
  2. [`docs/`](docs/): Detailed guides
  