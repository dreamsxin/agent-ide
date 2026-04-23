# AI Workflow V3

This file defines how future implementation runs should work after the Rust-first architecture reset.

## Read Order

Every new implementation run should read:

1. `AI_AGENT_IDE_PLAN.md`
2. `docs/ARCHITECTURE.md`
3. `docs/DESKTOP_IDE.md`
4. `docs/NEXT_STEPS.md`

Read `README.md` too once it exists or is updated.

## Operating Rule

If old code or old notes conflict with the V3 documents:

- trust the V3 documents
- treat the old shape as migration residue

## Default Loop

Each run should:

1. identify the earliest incomplete V3 task
2. implement or scaffold the smallest useful slice
3. run verification when feasible
4. update the V3 documents if boundaries changed
5. leave the next task explicit in `docs/NEXT_STEPS.md`

## Execution Priorities

Always prioritize in this order:

1. establish the Rust-first architecture skeleton
2. preserve a runnable minimal desktop IDE loop
3. keep protocol ownership explicit
4. only then expand advanced Agent capabilities

## Things To Avoid

- do not invest in legacy naming as if it were final
- do not move trusted runtime behavior into the frontend
- do not introduce major features before the workspace loop works
- do not adopt VS Code source as the repository implementation base
- do not let ad hoc Tauri commands replace a coherent runtime module structure

## Documentation Rule

After any architectural task, update:

- `AI_AGENT_IDE_PLAN.md`
- `docs/ARCHITECTURE.md`
- `docs/NEXT_STEPS.md`

Update `docs/DESKTOP_IDE.md` whenever desktop boundary decisions change.

## Verification Rule

Use the current verification commands when they exist, but do not let tooling drift become architectural authority.

The migration goal is:

- V3 docs first
- V3 structure second
- runnable desktop loop third
- advanced runtime capabilities after foundation
