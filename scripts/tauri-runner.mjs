import { spawnSync } from "node:child_process";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { tauriDevAddress } from "./tauri-dev-config.mjs";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, "..");
const cli = resolve(repoRoot, "node_modules", "@tauri-apps", "cli", "tauri.js");
const args = process.argv.slice(2);
const forwardedArgs = args.length > 0 ? args : ["dev"];
const env = { ...process.env };

if (forwardedArgs[0] === "dev" && !env.LY_WEB_DEV_SERVER) {
  env.LY_WEB_DEV_SERVER = tauriDevAddress;
}

const result = spawnSync(process.execPath, [cli, ...forwardedArgs], {
  stdio: "inherit",
  env,
});

if (result.error) {
  throw result.error;
}

process.exit(result.status ?? 1);
