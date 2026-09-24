"""Classifies the contract changes between two releases.

Usage: classify.py API_CHANGES OLD_CLI NEW_CLI CHANGES_OUT BUMP_OUT

API_CHANGES is `oasdiff changelog --format json` between the two OpenAPI
documents. OLD_CLI and NEW_CLI are the two `cli.json` files. CHANGES_OUT
receives one line per change, and BUMP_OUT the smallest bump that covers
them: `major`, `minor` or `patch`.
"""

import json
import sys

# oasdiff's level for a breaking change.
BREAKING = 3


def api_changes(changelog):
    for change in changelog:
        # This derivation picks the version, so a notice that the old
        # document's version is not bumped yet is not a change.
        if change["id"] == "api-version-not-bumped":
            continue
        level = "major" if change["level"] == BREAKING else "minor"
        yield level, f"{change['operation']} {change['path']}: {change['text']}"


def flatten(command, path="nit"):
    """Maps each command and each argument to its syntax.

    An argument's key is its command path and the flag or position that a
    caller types.
    """
    entries = {path: {}}
    for arg in command["args"]:
        if arg["long"]:
            name = f"--{arg['long']}"
        elif arg["short"]:
            name = f"-{arg['short']}"
        else:
            name = f"#{arg['index']}"
        entries[f"{path} {name}"] = arg
    for name, sub in command["subcommands"].items():
        entries |= flatten(sub, f"{path} {name}")
    return entries


def breaks_callers(old, new):
    """Whether a caller of `old` can fail against `new`."""
    return (
        set(old["possible_values"]) - set(new["possible_values"])
        or (new["required"] and not old["required"])
        or (old["short"] is not None and old["short"] != new["short"])
        or old["takes_value"] != new["takes_value"]
        or old["multiple"] != new["multiple"]
        or old["default_values"] != new["default_values"]
    )


def cli_changes(old_cli, new_cli):
    old, new = flatten(old_cli), flatten(new_cli)
    for key in sorted(old.keys() - new.keys()):
        yield "major", f"removed {key}"
    for key in sorted(new.keys() - old.keys()):
        if new[key].get("required"):
            yield "major", f"added required {key}"
        else:
            yield "minor", f"added {key}"
    for key in sorted(old.keys() & new.keys()):
        if old[key] != new[key]:
            level = "major" if breaks_callers(old[key], new[key]) else "minor"
            yield level, f"changed {key}"


def main(api_path, old_cli_path, new_cli_path, changes_out, bump_out):
    def load(path):
        with open(path) as f:
            return json.load(f)

    changes = [
        *api_changes(load(api_path)),
        *cli_changes(load(old_cli_path), load(new_cli_path)),
    ]
    with open(changes_out, "w") as f:
        f.writelines(f"{level}: {text}\n" for level, text in changes)
    levels = {level for level, _ in changes}
    bump = "major" if "major" in levels else "minor" if levels else "patch"
    with open(bump_out, "w") as f:
        f.write(f"{bump}\n")


if __name__ == "__main__":
    main(*sys.argv[1:])
