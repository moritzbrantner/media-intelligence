# ADR 0001: Bounded-Context Monorepo Split

## Status

Accepted

## Decision

The repository is split into bounded-context subprojects inside one monorepo:

- `contracts/`
- `projects/discovery/`
- `projects/extraction/`
- `projects/understanding/`
- `projects/search/`
- `ops/`

The legacy `services/` package remains temporarily as a compatibility layer.

## Rationale

- Shared contracts were changing too quickly for immediate multi-repo extraction.
- The implemented codebase was still small enough that hard repo boundaries would add more release overhead than autonomy.
- Tests were coupled to repository-root import behavior and needed containment before extraction.

## Consequences

- New code should be added under `projects/*/src/` or `contracts/src/`.
- Shared payload types should be added to `mi_contracts` first, then consumed downstream.
- `services/*` imports are transitional and should be removed after the next migration milestone.
