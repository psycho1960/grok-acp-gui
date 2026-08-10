import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { basename, dirname, extname, join, relative, resolve } from "node:path";
import process from "node:process";

const root = resolve(import.meta.dirname, "..");
const tauriConfigPath = join(root, "src-tauri", "tauri.conf.json");
const cargoManifestPath = join(root, "src-tauri", "Cargo.toml");
const packagePath = join(root, "package.json");

function fail(message) {
  throw new Error(`GAG-016 release gate: ${message}`);
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function readCargoVersion() {
  const cargo = readFileSync(cargoManifestPath, "utf8");
  const packageBlock = cargo.match(/\[package\]([\s\S]*?)(?:\n\[|$)/)?.[1];
  const version = packageBlock?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!version) fail("Cargo package version is missing");
  return version;
}

function verifyConfiguration() {
  const tauri = readJson(tauriConfigPath);
  const pkg = readJson(packagePath);
  const cargoVersion = readCargoVersion();
  const bundle = tauri.bundle ?? {};
  const windows = bundle.windows ?? {};
  const targets = bundle.targets ?? [];

  if (pkg.version !== tauri.version || cargoVersion !== tauri.version) {
    fail(`version mismatch (npm=${pkg.version}, Cargo=${cargoVersion}, Tauri=${tauri.version})`);
  }
  if (tauri.identifier !== "com.grokacpgui.desktop") fail("application identifier changed");
  if (JSON.stringify(targets) !== JSON.stringify(["nsis", "msi"])) {
    fail("bundle targets must remain exactly x64 NSIS and MSI");
  }
  if (!bundle.publisher || !bundle.icon?.includes("icons/icon.ico")) {
    fail("publisher and Windows icon metadata are required");
  }
  if (windows.allowDowngrades !== false) fail("installer downgrade blocking must remain enabled");
  if (windows.digestAlgorithm !== "sha256") fail("Windows signing digest must be sha256");
  if (windows.webviewInstallMode?.type !== "downloadBootstrapper") {
    fail("WebView2 bootstrapper mode is required for the Windows candidate");
  }
  if (windows.nsis?.installMode !== "currentUser") {
    fail("NSIS must support installation by a standard user");
  }
  if (windows.wix?.upgradeCode !== "59b1c3c7-7027-5376-86b5-69993d342750") {
    fail("the frozen MSI upgrade code changed");
  }
  if (bundle.createUpdaterArtifacts) fail("remote updater artifacts are outside GAG-016 scope");

  console.log(`GAG-016 packaging configuration verified for ${tauri.productName} ${tauri.version}.`);
  return { tauri, version: tauri.version };
}

function collectInstallerArtifacts(directory) {
  if (!existsSync(directory)) fail(`bundle directory does not exist: ${directory}`);
  const artifacts = [];
  const visit = (current) => {
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      const path = join(current, entry.name);
      if (entry.isDirectory()) visit(path);
      else if ([".exe", ".msi"].includes(extname(entry.name).toLowerCase())) artifacts.push(path);
    }
  };
  visit(directory);
  if (!artifacts.some((path) => extname(path).toLowerCase() === ".exe")) fail("NSIS installer is missing");
  if (!artifacts.some((path) => extname(path).toLowerCase() === ".msi")) fail("MSI installer is missing");
  return artifacts.sort();
}

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function commitSha() {
  return execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim();
}

function sourceTreeState() {
  const status = execFileSync("git", ["status", "--porcelain", "--untracked-files=normal"], {
    cwd: root,
    encoding: "utf8",
  });
  return status.trim() ? "dirty" : "clean";
}

function authenticodeStatus(path) {
  if (process.platform !== "win32") return "not-verified-non-windows";
  const command = "Import-Module Microsoft.PowerShell.Security; (Get-AuthenticodeSignature -LiteralPath $env:GAG016_SIGNATURE_PATH).Status.ToString()";
  const systemModulePath = "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\Modules";
  try {
    const status = execFileSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", command], {
      encoding: "utf8",
      windowsHide: true,
      env: {
        ...process.env,
        GAG016_SIGNATURE_PATH: path,
        PSModulePath: `${systemModulePath};${process.env.PSModulePath ?? ""}`,
      },
      stdio: ["ignore", "pipe", "pipe"],
    }).trim();
    return status === "Valid" ? "signed-valid" : `unsigned-or-invalid:${status || "Unknown"}`;
  } catch {
    return "signature-check-failed";
  }
}

function dependencyInventory(outputDirectory) {
  const npmCli = process.env.npm_execpath ?? join(dirname(process.execPath), "node_modules", "npm", "bin", "npm-cli.js");
  if (!existsSync(npmCli)) fail(`npm CLI entry point does not exist: ${npmCli}`);
  const npmInventory = execFileSync(process.execPath, [npmCli, "ls", "--all", "--json"], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 32 * 1024 * 1024,
  });
  const cargoInventory = execFileSync("cargo", ["metadata", "--format-version", "1", "--locked", "--manifest-path", cargoManifestPath], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 32 * 1024 * 1024,
  });
  writeFileSync(join(outputDirectory, "dependencies-npm.json"), npmInventory);
  writeFileSync(join(outputDirectory, "dependencies-cargo.json"), cargoInventory);
}

function createManifest(args) {
  const { tauri, version } = verifyConfiguration();
  const positional = args.filter((value) => !value.startsWith("--"));
  const bundleDirectory = resolve(root, positional[0] ?? "src-tauri/target/x86_64-pc-windows-msvc/release/bundle");
  const outputDirectory = resolve(root, positional[1] ?? ".gag-016-evidence");
  const requireSigned = args.includes("--require-signed");
  const allowDirty = args.includes("--allow-dirty");
  const treeState = sourceTreeState();
  if (treeState !== "clean" && !allowDirty) {
    fail("artifact manifests require a clean fixed commit (use --allow-dirty only for development fixtures)");
  }
  mkdirSync(outputDirectory, { recursive: true });

  const sourceCommitSha = commitSha();

  const artifacts = collectInstallerArtifacts(bundleDirectory).map((path) => {
    const signingStatus = authenticodeStatus(path);
    if (requireSigned && signingStatus !== "signed-valid") fail(`${basename(path)} is not validly signed (${signingStatus})`);
    return {
      file: relative(bundleDirectory, path).replaceAll("\\", "/"),
      bytes: statSync(path).size,
      sha256: sha256(path),
      version,
      architecture: "x86_64-pc-windows-msvc",
      commitSha: sourceCommitSha,
      signingStatus,
    };
  });

  const manifest = {
    schemaVersion: 1,
    candidateType: treeState === "dirty"
      ? "development-unsigned-candidate"
      : requireSigned
        ? "signed-production-candidate"
        : "internal-unsigned-candidate",
    productName: tauri.productName,
    version,
    identifier: tauri.identifier,
    architecture: "x86_64-pc-windows-msvc",
    sourceTreeState: treeState,
    generatedAt: new Date().toISOString(),
    artifacts,
  };
  writeFileSync(join(outputDirectory, "artifact-manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  writeFileSync(
    join(outputDirectory, "checksums.sha256"),
    `${artifacts.map((artifact) => `${artifact.sha256} *${artifact.file}`).join("\n")}\n`,
  );
  dependencyInventory(outputDirectory);
  console.log(`Wrote GAG-016 evidence for ${artifacts.length} installers to ${outputDirectory}.`);
}

const [command = "verify", ...args] = process.argv.slice(2);
try {
  if (command === "verify") verifyConfiguration();
  else if (command === "manifest") createManifest(args);
  else fail(`unknown command: ${command}`);
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}
