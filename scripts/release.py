#!/usr/bin/env python3
"""Release Guildsmith in explicit stages. Requires Python 3.11+; see docs/release.md."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
REPO = "clarkscode/guildsmith"
REGISTRY = "https://registry.npmjs.org"
CRATES = (
    "guildsmith-core", "guildsmith-app", "guildsmith-discord",
    "guildsmith-mcp", "guildsmith",
)
TARGETS = {
    "linux-x64": "x86_64-unknown-linux-musl.tar.xz",
    "linux-arm64": "aarch64-unknown-linux-musl.tar.xz",
    "darwin-x64": "x86_64-apple-darwin.tar.xz",
    "darwin-arm64": "aarch64-apple-darwin.tar.xz",
    "win32-x64": "x86_64-pc-windows-msvc.zip",
}
PACKAGES = (*TARGETS, "guildsmith")


def run(*args, capture=False):
    args = [str(arg) for arg in args]
    print("+", shlex.join(args), flush=True)
    # npm is a .cmd launcher on Windows; cmd.exe is needed to start it.
    if os.name == "nt" and args[0] == "npm":
        args = ["cmd.exe", "/c", *args]
    result = subprocess.run(
        args, cwd=ROOT, check=True, text=True,
        stdout=subprocess.PIPE if capture else None,
    )
    return result.stdout.strip() if capture else None


def version():
    return tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]


def stable_version(value):
    if not re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", value):
        raise ValueError("Use a stable version such as 0.1.1 (no v prefix or prerelease).")
    return tuple(map(int, value.split(".")))


def clean():
    if run("git", "status", "--porcelain", capture=True):
        raise ValueError("Commit or stash your changes first; the working tree must be clean.")


def json_bytes(doc):
    return json.dumps(doc, indent=2, ensure_ascii=False) + "\n"


def replace_once(text, pattern, replacement):
    text, count = re.subn(pattern, lambda _: replacement, text, flags=re.MULTILINE)
    if count != 1:
        raise ValueError(f"Expected one match while updating version, found {count}: {pattern}")
    return text


def version_updates(new):
    """Compute all changes before writing; preserve TOML comments and unrelated JSON fields."""
    old = version()
    if stable_version(new) <= stable_version(old):
        raise ValueError(f"New version must be greater than {old}.")
    changes = {}
    cargo = (ROOT / "Cargo.toml").read_text()
    old_line = f'version = "{old}"'
    cargo = replace_once(cargo, rf"^{re.escape(old_line)}$", f'version = "{new}"')
    for name in CRATES[:-1]:
        line = next(line for line in cargo.splitlines() if line.startswith(f"{name} = "))
        updated = line.replace(f'version = "{old}"', f'version = "{new}"')
        if updated == line:
            raise ValueError(f"Unexpected internal dependency version: {name}")
        cargo = cargo.replace(line, updated, 1)
    tomllib.loads(cargo)
    changes["Cargo.toml"] = cargo

    lock = (ROOT / "Cargo.lock").read_text()
    for name in CRATES:
        block = rf'(^name = "{re.escape(name)}"\n)version = "{re.escape(old)}"'
        lock = replace_once(lock, block, f'name = "{name}"\nversion = "{new}"')
    tomllib.loads(lock)
    changes["Cargo.lock"] = lock

    for path in [ROOT / "npm/guildsmith/package.json", *sorted((ROOT / "npm/platforms").glob("*/package.json"))]:
        doc = json.loads(path.read_text())
        doc["version"] = new
        for name in doc.get("optionalDependencies", {}):
            if name.startswith("@guildsmith/"):
                doc["optionalDependencies"][name] = new
        changes[str(path.relative_to(ROOT))] = json_bytes(doc)

    for directory, manifest in (
        ("codex", ".codex-plugin/plugin.json"),
        ("claude-code", ".claude-plugin/plugin.json"),
    ):
        rel = f"plugins/{directory}/{manifest}"
        doc = json.loads((ROOT / rel).read_text())
        doc["version"] = new
        changes[rel] = json_bytes(doc)
        rel = f"plugins/{directory}/.mcp.json"
        doc = json.loads((ROOT / rel).read_text())
        doc["mcpServers"]["guildsmith"]["args"] = ["-y", f"guildsmith@{new}", "mcp"]
        changes[rel] = json_bytes(doc)
    return changes


def check_crate_keywords():
    """Check crates.io keyword restrictions before starting a release."""
    for name in CRATES:
        path = ROOT / "crates" / name / "Cargo.toml"
        keywords = tomllib.loads(path.read_text())["package"].get("keywords", [])
        if len(keywords) > 5:
            raise ValueError(f"{name}: crates.io allows at most five keywords.")
        for keyword in keywords:
            if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_+-]{0,19}", keyword):
                raise ValueError(
                    f"{name}: invalid crates.io keyword {keyword!r}; use 1–20 ASCII "
                    "letters, digits, _, - or +, starting with a letter or digit."
                )


def check_versions():
    check_crate_keywords()
    expected = version()
    stable_version(expected)
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]
    for name in CRATES[:-1]:
        if workspace["dependencies"][name]["version"] != expected:
            raise ValueError(f"{name}: internal dependency version differs from {expected}")
    for package in PACKAGES:
        rel = "npm/guildsmith/package.json" if package == "guildsmith" else f"npm/platforms/{package}/package.json"
        doc = json.loads((ROOT / rel).read_text())
        if doc["version"] != expected:
            raise ValueError(f"{rel}: version differs from {expected}")
        if package == "guildsmith":
            wanted = {f"@guildsmith/{name}": expected for name in TARGETS}
            if doc["optionalDependencies"] != wanted:
                raise ValueError("npm launcher platform dependencies differ from the release version.")
    run("cargo", "run", "--locked", "--quiet", "--package", "xtask", "--", "check-plugins")


def check():
    check_versions()
    commands = [
        (sys.executable, "scripts/test_release.py"),
        ("cargo", "fmt", "--all", "--check"),
        ("cargo", "clippy", "--workspace", "--all-targets", "--all-features", "--locked", "--", "-D", "warnings"),
        ("cargo", "test", "--workspace", "--all-features", "--locked"),
        ("cargo", "xtask", "schema", "--check"),
        ("node", "--check", "npm/guildsmith/bin/guildsmith.js"),
        ("cargo", "build", "--profile", "dist", "--package", "guildsmith", "--locked"),
        ("cargo", "xtask", "budgets", "--bin", f"target/dist/guildsmith{'.exe' if os.name == 'nt' else ''}"),
        ("dist", "generate", "--mode", "ci", "--check"),
        ("dist", "plan"),
    ]
    for command in commands:
        run(*command)
    print("Local checks passed. Live Discord tests need separate opt-in; see docs/release.md.")


def tag(execute):
    release = f"v{version()}"
    stable_version(version())
    if not execute:
        print(f"Preview: create and push {release} from HEAD after checking clean state and successful CI.")
        print("This starts GitHub Releases and crates.io publication (environment approval required).")
        print("Existing tags are never overwritten. Add --execute to proceed.")
        return
    clean()
    check_versions()
    sha = run("git", "rev-parse", "HEAD", capture=True)
    remote = run("git", "ls-remote", "origin", "refs/heads/main", capture=True)
    if not remote or remote.split()[0] != sha:
        raise ValueError("HEAD must match origin/main on GitHub. Push the reviewed commit first.")
    if run("git", "tag", "--list", release, capture=True) or run(
        "git", "ls-remote", "origin", f"refs/tags/{release}", capture=True
    ):
        raise ValueError(f"{release} already exists. Never overwrite it automatically; see docs/release.md.")
    runs = json.loads(run(
        "gh", "run", "list", "--repo", REPO, "--workflow", "ci.yml", "--commit", sha,
        "--limit", "20", "--json", "headSha,conclusion,event", capture=True,
    ))
    if not any(r["headSha"] == sha and r["conclusion"] == "success" and r["event"] == "push" for r in runs):
        raise ValueError("No successful push CI run for this exact commit. Wait for CI before tagging.")
    run("git", "tag", "-a", release, sha, "-m", f"Release {version()}")
    run("git", "push", "origin", f"refs/tags/{release}")


def require_release_checkout():
    clean()
    release = f"v{version()}"
    sha = run("git", "rev-parse", "HEAD", capture=True)
    if run("git", "rev-parse", f"{release}^{{commit}}", capture=True) != sha:
        raise ValueError(f"Check out {release} first: npm templates must come from the released commit.")
    refs = run("git", "ls-remote", "origin", f"refs/tags/{release}", f"refs/tags/{release}^{{}}", capture=True)
    commits = dict(line.split()[::-1] for line in refs.splitlines())
    remote_sha = commits.get(f"refs/tags/{release}^{{}}", commits.get(f"refs/tags/{release}"))
    if remote_sha != sha:
        raise ValueError("Local release tag differs from the published remote tag.")
    return release


def verify_archive(archive, checksum):
    parts = checksum.read_text().split()
    if not parts or not re.fullmatch(r"[a-fA-F0-9]{64}", parts[0]):
        raise ValueError(f"Invalid SHA-256 file: {checksum}")
    with archive.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    if digest != parts[0].lower():
        raise ValueError(f"Checksum mismatch: {archive}")


def stage_npm():
    release = require_release_checkout()
    check_versions()
    base = ROOT / "target/release-helper"
    base.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix=f"{release}-", dir=base))
    archives = [f"guildsmith-{target}" for target in TARGETS.values()]
    command = ["gh", "release", "download", release, "--repo", REPO, "--dir", str(work)]
    for archive in archives:
        command.extend(["--pattern", archive, "--pattern", f"{archive}.sha256"])
    run(*command)
    for archive in archives:
        verify_archive(work / archive, work / f"{archive}.sha256")
    run("cargo", "xtask", "npm-pack", work, "--out", work / "packages")
    tarballs = work / "tarballs"
    tarballs.mkdir()
    packed = []
    for name in PACKAGES:
        directory = work / "packages" / name
        result = json.loads(run(
            "npm", "pack", directory, "--ignore-scripts", "--json",
            "--pack-destination", tarballs, capture=True,
        ))[0]
        expected_name = "guildsmith" if name == "guildsmith" else f"@guildsmith/{name}"
        if result["name"] != expected_name or result["version"] != version():
            raise ValueError(f"Unexpected npm package identity: {result['name']}@{result['version']}")
        packed.append((expected_name, tarballs / result["filename"]))
    print(f"Verified and packed all six npm packages in {tarballs}")
    return packed


def npm_exists(name):
    """Only a registry E404 means missing; authentication/network failures must stop the release."""
    command = ["npm", "view", f"{name}@{version()}", "version", "--json", "--registry", REGISTRY]
    if os.name == "nt":
        command = ["cmd.exe", "/c", *command]
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    if result.returncode == 0:
        if json.loads(result.stdout) != version():
            raise ValueError(f"Unexpected registry version for {name}")
        return True
    for output in (result.stdout, result.stderr):
        try:
            if json.loads(output).get("error", {}).get("code") == "E404":
                return False
        except (json.JSONDecodeError, AttributeError):
            pass
    raise ValueError(f"Could not check {name} on npm; publication stopped. Check network and npm login.")


def publish_npm(execute):
    if not execute:
        print("Preview: download release archives, verify checksums, pack and publish five platform")
        print("packages, then guildsmith. Uses your npm login and interactive 2FA; no stored CI token.")
        print("Run npm-stage to inspect the packages first. Add --execute to publish publicly.")
        return
    run("npm", "whoami", "--registry", REGISTRY)
    packages = stage_npm()
    # Check every registry lookup before publishing the first new package.
    missing = [(name, path) for name, path in packages if not npm_exists(name)]
    for name, path in missing:
        print(f"Publishing {name}@{version()}; complete npm's authentication prompts.", flush=True)
        run("npm", "publish", path, "--access", "public", "--ignore-scripts", "--registry", REGISTRY)
    print("All six npm versions are published. Run smoke --execute next.")


def smoke(execute):
    command = ["gh", "workflow", "run", "install-smoke.yml", "--repo", REPO, "-f", f"version={version()}"]
    if execute:
        run(*command)
    else:
        print("Preview:", shlex.join(command))
        print("Add --execute to run the installation checks on GitHub.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command")
    sub.add_parser("check", help="Run local checks; never publishes (default)")
    prepare = sub.add_parser("version", help="Synchronize Rust, npm and both agent plugin versions")
    prepare.add_argument("version")
    sub.add_parser("npm-stage", help="Download, verify and pack the current tagged GitHub release")
    for name, help_text in (
        ("tag", "Preview starting a release from a committed, CI-tested main"),
        ("npm-publish", "Preview the first npm publication using your local login and 2FA"),
        ("smoke", "Preview the GitHub installation checks"),
    ):
        command = sub.add_parser(name, help=help_text)
        command.add_argument("--execute", action="store_true", help="Perform the displayed remote action")
    args = parser.parse_args()
    if args.command in (None, "check"):
        check()
    elif args.command == "version":
        clean()
        changes = version_updates(args.version)
        for rel, text in changes.items():
            (ROOT / rel).write_text(text)
            print("updated", rel)
        print("Update CHANGELOG.md, run check, review the diff, then commit and push main.")
    elif args.command == "npm-stage":
        stage_npm()
    else:
        {"tag": tag, "npm-publish": publish_npm, "smoke": smoke}[args.command](args.execute)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"Release stopped: {error}", file=sys.stderr)
        sys.exit(1)
