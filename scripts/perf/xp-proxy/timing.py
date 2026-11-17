"""Measure consumer budget through artifact upload; final log remains authoritative."""

import json
import os
from pathlib import Path
import sys
import time

elapsed = time.time() - int(os.environ["PERF_JOB_STARTED"])
budget = 360 if os.environ["PROFILE"] == "full" else 180
value = dict(
    stage=sys.argv[1],
    seconds=elapsed,
    design_budget_seconds=budget,
    budget_warning=elapsed > budget,
)
print(json.dumps(value))
if elapsed > budget:
    print(
        f"::warning::Performance consumer exceeds {budget}s design budget ({elapsed:.1f}s)"
    )
if sys.argv[1] == "before-upload":
    Path("evidence/timing.json").write_text(json.dumps(value))
with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as summary:
    summary.write(
        f"\nConsumer {sys.argv[1]}: {elapsed:.1f}s / {budget}s design budget.\n"
    )
