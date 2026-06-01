"use strict";

const { execFileSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const crateDir = path.resolve(__dirname, "..");
const metadata = JSON.parse(
  execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], {
    cwd: crateDir,
    encoding: "utf8",
  }),
);

const profile = process.env.JUST_BASH_NAPI_PROFILE || "debug";
const extensionByPlatform = {
  darwin: "dylib",
  linux: "so",
  win32: "dll",
};
const extension = extensionByPlatform[process.platform];

if (!extension) {
  throw new Error(`Unsupported platform for just-bash-napi: ${process.platform}`);
}

const libraryName =
  process.platform === "win32"
    ? `just_bash_napi.${extension}`
    : `libjust_bash_napi.${extension}`;
const source = path.join(metadata.target_directory, profile, libraryName);

if (!fs.existsSync(source)) {
  throw new Error(
    `Built Just Bash addon library not found at ${source}. Run cargo build -p just-bash-napi first.`,
  );
}

const destination = path.join(
  crateDir,
  `just-bash-napi.${process.platform}-${process.arch}.node`,
);

fs.copyFileSync(source, destination);
console.log(`Copied ${source} to ${destination}`);
