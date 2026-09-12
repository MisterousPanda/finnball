# FINNBALL interchange

`finnball.json` is generated from the Rust source of truth (`src/sim.rs`, `src/roster.rs`, `src/arenas.rs`). Do not edit it by hand.

```bash
UPDATE_INTEROP=1 cargo test interop::
python3 scripts/finnball_dcc.py validate
```

Unity, Unreal, and Blender install steps: [docs/dcc.md](../docs/dcc.md).
