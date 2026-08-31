import * as fs from "node:fs";

export type TomlSection = "package" | "workspace.package";

/**
 * Bumps the `version` field inside the specified TOML section.
 * Uses a regex that is scoped to the section's content before the next
 * top-level `[` header, avoiding accidental matches in other sections.
 */
export function bumpTomlVersion(
  filePath: string,
  version: string,
  section: TomlSection
): void {
  const sectionKey =
    section === "workspace.package" ? "workspace\\.package" : "package";
  const pattern = new RegExp(
    `(\\[${sectionKey}\\][\\s\\S]*?version\\s*=\\s*")[^"]+(")`,
    "m"
  );
  const content = fs.readFileSync(filePath, "utf-8");
  const updated = content.replace(pattern, `$1${version}$2`);
  if (updated === content) {
    throw new Error(
      `Could not find version field in [${section}] section of ${filePath}`
    );
  }
  fs.writeFileSync(filePath, updated);
  console.log(`  Bumped ${filePath} [${section}] version to ${version}`);
}
