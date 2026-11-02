# Modular floating dock (sample)

An 8 ft x 16 ft floating dock: 24 in rotomolded floats, a two-layer 6061
aluminum tube frame (joists on the floats, a stringer on every module line),
ipe decking, six stainless cleats, PVC end bumpers, and a 12 ft aluminum
access gangway. The gangway hinges on the stern deck edge through a
revolute mate driven by `gangway_angle_deg` (shore end up).

`liquidcad.json` holds parameters, materials, and the feature graph. It is
generated from `liquidcad_core::templates::modular_dock` and a test fails if
the two drift apart. Regenerate it with:

```bash
cargo run -p liquidcad-cli -- new-dock projects/modular-dock/liquidcad.json
```

The desktop app opens this file read-only; Save writes a copy elsewhere.

The file uses project schema 2 (user features and mates). Schema 1 files
load and are upgraded in memory.

Rules the engine enforces:

- Width and length are whole multiples of the float module (24 or 48 in).
- At least 2 x 2 modules.
- Cleat placements stay on the deck and snap to 0.5 in.
- The gangway slope stays within its mate limits, -20 to 30 degrees.
