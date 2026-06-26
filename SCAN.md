# Headless batch scan (`scan` subcommand)

The GUI binary doubles as a CLI. `scan` costs every STL/OBJ in a folder and
upserts the result into the shared `piece_costs` catalog that OrderTracker reads.

```bash
jewelry_cost_calculator scan <DIR> \
  [--kind ring|pendant|auto] \   # default: auto
  [--sizes 5-10] \               # ring size range to publish (rings only)
  [--recursive] \                # descend into subfolders
  [--dry-run] \                  # compute, do not write to the DB
  [--offline] \                  # skip the live price fetch, use defaults
  [--wax-cost 0.10] \            # wax USD/gram
  [--report scan-report.json]    # write a per-file JSON report
```

Requires `SURREAL_URL` (and optional `SURREAL_USER`/`SURREAL_PASS`) in env or
`.env` for publishing; `--dry-run` needs neither. Live metal prices use the
build-time `METAL_API_KEY`; on fetch failure it falls back to defaults.

## Ring size handling

Ring size is taken from the **filename** (e.g. `celtic-knot-size7.stl`,
`dragon_sz9`, `band US 10.5`), not from geometry — auto hole-detection is
unreliable on complex rings. The filename size's known inner diameter anchors the
`--sizes` range scaling.

- `--kind ring`: files with **no parseable size are skipped and listed** in the
  report (`status: "skipped: no ring size in filename"`) for manual review.
- `--kind pendant`: one row per file at native volume (`ring_size = N/A`).
- `--kind auto`: a filename with a size → ring; otherwise → pendant.

## Driving it from Claude/cowork against Google Drive

1. Claude lists STL/OBJ files in the target Google Drive folder via the Google
   Drive connector and downloads them to a local folder.
2. Run the rings folder and pendants folder separately:
   ```bash
   jewelry_cost_calculator scan ./drive/rings    --kind ring    --sizes 5-10 --report rings.json
   jewelry_cost_calculator scan ./drive/pendants --kind pendant               --report pendants.json
   ```
3. Claude reads `*.json`, surfaces `skipped`/`error` items so you can rename the
   offending files (add the size) and re-run just those.
4. OrderTracker reads `piece_costs` and shows cost vs. sale price live.
