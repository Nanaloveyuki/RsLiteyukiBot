import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, "..");
const tauriConfigPath = resolve(repoRoot, "src-tauri", "tauri.conf.json");
const tauriConfig = JSON.parse(readFileSync(tauriConfigPath, "utf8"));
const devUrl = new URL(tauriConfig.build.devUrl);
const defaultPort = devUrl.protocol === "https:" ? 443 : 80;

export const tauriDevUrl = tauriConfig.build.devUrl;
export const tauriDevHost = devUrl.hostname;
export const tauriDevPort = Number(devUrl.port || defaultPort);
export const tauriDevAddress = `${tauriDevHost}:${tauriDevPort}`;
