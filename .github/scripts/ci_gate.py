"""Fail closed unless every test/check job succeeded for this workflow revision."""
import json
import os
import sys

MANDATORY = {
    "changes", "formatting", "translation-parity", "design-ratchet",
    "frontend-check", "frontend-tests", "rust-check", "browser-e2e",
    "http-client-policy", "native-secret-store", "server-secret-store",
}
MOBILE = {"android-check": "android", "ios-check": "ios"}


def failures(needs):
    problems = []
    for job in sorted(MANDATORY):
        result = needs.get(job, {}).get("result", "missing")
        if result != "success":
            problems.append(f"{job}: {result}")
    changes = needs.get("changes", {}).get("outputs", {})
    for job, platform in MOBILE.items():
        required = changes.get(platform)
        result = needs.get(job, {}).get("result", "missing")
        if required not in ("true", "false"):
            problems.append(f"{job}: missing/invalid change classification")
        elif result != "success" and not (required == "false" and result == "skipped"):
            problems.append(f"{job}: {result} (required={required})")
    # A newly added dependency cannot silently fail outside the known matrix.
    for job in sorted(needs.keys() - MANDATORY - MOBILE.keys()):
        if needs[job].get("result") != "success":
            problems.append(f"{job}: {needs[job].get('result', 'missing')}")
    return problems


if __name__ == "__main__":
    problems = failures(json.loads(os.environ["NEEDS_JSON"]))
    if problems:
        print("Required CI did not pass:\n" + "\n".join(problems), file=sys.stderr)
        sys.exit(1)
    print("All required tests and checks passed for this tested revision.")
