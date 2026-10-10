"""Checks that a release version covers its contract changes.

Usage: gate.py OLD NEW KIND

OLD is the version of the last release, NEW the version of the release
candidate, and KIND the kind of the contract changes between them, as
classify.py writes it. Exits non-zero with the reason when NEW is not
the next patch, minor or major version of OLD, or when it is a smaller
step than KIND needs.
"""

import re
import sys


def parse(version):
    match = re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", version)
    if not match:
        sys.exit(f"{version} is not a release version MAJOR.MINOR.PATCH")
    return tuple(int(part) for part in match.groups())


def main(old, new, kind):
    major, minor, patch = parse(old)
    steps = {
        (major, minor, patch + 1): "patch",
        (major, minor + 1, 0): "minor",
        (major + 1, 0, 0): "major",
    }
    step = steps.get(parse(new))
    if step is None:
        sys.exit(f"{new} is not the next patch, minor or major version of {old}")
    # Before 1.0.0, Cargo treats 0.MINOR as the compatible line, so a
    # breaking or additive change needs one step less.
    needed = {
        "breaking": "minor" if major == 0 else "major",
        "additive": "patch" if major == 0 else "minor",
        "none": "patch",
    }[kind]
    order = ["patch", "minor", "major"]
    if order.index(step) < order.index(needed):
        sys.exit(f"{kind} changes need a {needed} step from {old}, not {new}")


if __name__ == "__main__":
    main(*sys.argv[1:])
