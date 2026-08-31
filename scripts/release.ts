/**
 * Release script for shaide_server (Rust workspace).
 *
 * Usage:
 *   npx tsx scripts/release.ts <version> [changelog-file] [pr-body-file] [--dry-run]
 *
 * Creates a release branch, bumps Cargo.toml, updates CHANGELOG, opens a PR.
 * Merging the PR triggers CI to create the tag, push Docker images, and create the GitHub Release.
 */

import * as fs from "node:fs";
import { exec, currentBranch, isClean, tagExists } from "./lib/git";
import { insertChangelog } from "./lib/changelog";
import { bumpTomlVersion } from "./lib/toml";

const args = process.argv.slice(2);
if (args.length < 1 || args[0] === "--help") {
  console.error("Usage: npx tsx scripts/release.ts <version> [changelog-file] [pr-body-file] [--dry-run]");
  process.exit(1);
}

const dryRun = args.includes("--dry-run");
const positional = args.filter((a) => !a.startsWith("--"));
const version = positional[0].startsWith("v") ? positional[0] : `v${positional[0]}`;
const versionBare = version.slice(1);
const changelogFile = positional[1] ?? null;
const prBodyFile = positional[2] ?? null;

const snippet =
  changelogFile && fs.existsSync(changelogFile)
    ? fs.readFileSync(changelogFile, "utf-8").trim()
    : "";

console.log(`\nshaide_server release ${version}${dryRun ? " [dry-run]" : ""}`);
console.log("\nPreparing release branch");

if (!isClean()) {
  console.error("Working tree is not clean. Commit or stash changes first.");
  process.exit(1);
}
if (tagExists(version)) {
  console.error(`Tag ${version} already exists.`);
  process.exit(1);
}
if (currentBranch() !== "main") {
  console.error("Must be on main before creating a release branch.");
  process.exit(1);
}

const branch = `release/${version}`;
exec("git", ["checkout", "-b", branch], dryRun);

if (dryRun) {
  console.log(`  [dry-run] bumpTomlVersion Cargo.toml [workspace.package] → ${versionBare}`);
} else {
  bumpTomlVersion("Cargo.toml", versionBare, "workspace.package");
}

if (snippet) {
  if (dryRun) {
    console.log(`  [dry-run] insertChangelog CHANGELOG.md for ${version}`);
  } else {
    insertChangelog("CHANGELOG.md", snippet, version);
    console.log("  Updated CHANGELOG.md");
  }
} else {
  console.warn("  Warning: no changelog snippet provided – CHANGELOG.md not updated.");
}

const filesToAdd = ["Cargo.toml"];
if (snippet) filesToAdd.push("CHANGELOG.md");
exec("git", ["add", ...filesToAdd], dryRun);
exec("git", ["commit", "-m", `chore(release): ${version}`], dryRun);
exec("git", ["push", "origin", branch], dryRun);

const ghArgs = ["pr", "create", "--title", `RELEASE ${version}`, "--base", "main"];
if (prBodyFile && fs.existsSync(prBodyFile)) {
  ghArgs.push("--body-file", prBodyFile);
} else {
  const prBody = [`Release ${version}`, "", snippet ? "## Changes" : "", snippet]
    .filter((l) => l !== undefined)
    .join("\n")
    .trim();
  ghArgs.push("--body", prBody);
}
exec("gh", ghArgs, dryRun);

console.log(`\nPR created for ${branch}.`);
console.log("After merging, CI will automatically create the tag, push Docker images, and create the GitHub Release.");
