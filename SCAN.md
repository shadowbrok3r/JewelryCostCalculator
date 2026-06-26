# Headless batch scan (`scan` subcommand)

The GUI binary doubles as a CLI. `scan` costs every STL/OBJ in a folder and
upserts the result into the shared `piece_costs` catalog that OrderTracker reads.

```bash
jewelry_cost_calculator scan <DIR> \
  [--kind ring|pendant|auto] \   # default: auto
  [--recursive] \                # descend into subfolders (rings live in per-design subfolders)
  [--dry-run] \                  # compute, do not write to the DB
  [--offline] \                  # skip the live price fetch, use defaults
  [--wax-cost 0.10] \            # wax USD/gram
  [--report scan-report.json]    # write a per-file JSON report
```

Requires `SURREAL_URL` (and optional `SURREAL_USER`/`SURREAL_PASS`) in env or
`.env` for publishing; `--dry-run` needs neither. Live metal prices use the
build-time `METAL_API_KEY`; on fetch failure it falls back to defaults.

## Ring size handling

Rings are stored one file per size, with the size appended to the design name
(`Hades9.stl`, `Hades8.75.stl`, `Kamon-11.25.stl`, `AthenaRing8.obj`). For each
file the scanner:

- parses the **last in-range number (3–16)** as the size — quarter sizes
  (`8.75`, `11.25`) are kept;
- uses the text before it as the catalog **design_key** (`Hades9` -> `Hades`),
  so every size groups under one design;
- costs the file at its **native volume** (the mesh is already that size) and
  writes one row: `(design_key, "US <size>")`.

Files with **no parseable ring size** (`AthenaRing.stl`, `UMesh_AthenaRing.obj`,
`BlankSignet.obj`) are **skipped and listed** in the report for manual review.

- `--kind ring`: every file must yield a size, else it's skipped + logged.
- `--kind pendant`: one row per file at native volume (`design_key` = filename
  stem, `ring_size` = N/A).
- `--kind auto`: a filename with an in-range size → ring, otherwise → pendant.

## Driving it against Google Drive

The Drive files are large (7–345 MB each), so they are synced to local disk with
`rclone` (or Google Drive for Desktop), then scanned in place:

```bash
rclone copy "gdrive:3D/Casting/RING"     ./drive/rings    --transfers=8
rclone copy "gdrive:3D/Casting/PENDANTS" ./drive/pendants --transfers=8

jewelry_cost_calculator scan ./drive/rings    --kind ring --recursive --report rings.json
jewelry_cost_calculator scan ./drive/pendants --kind pendant          --report pendants.json
```

Review `skipped`/`error` items in the JSON (e.g. a ring whose filename lacks a
size), fix the names, and re-run. OrderTracker reads `piece_costs` and shows
cost vs. sale price live.
